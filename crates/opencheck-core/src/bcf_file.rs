//! BCF 2.1 files (`.bcfzip`): topics with markup, viewpoint and snapshot,
//! for exchanging issues without a BCF server.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use crate::bcf::ViewpointRequest;

/// One topic of a BCF file.
#[derive(Debug, Clone, Default)]
pub struct FileTopic {
    pub guid: String,
    pub title: String,
    pub topic_type: String,
    pub topic_status: String,
    pub priority: String,
    pub index: Option<u32>,
    pub labels: Vec<String>,
    /// ISO 8601, e.g. `2026-10-20T10:00:00Z`.
    pub creation_date: String,
    pub creation_author: String,
    pub modified_date: String,
    pub assigned_to: String,
    pub description: String,
    pub viewpoint: Option<FileViewpoint>,
}

#[derive(Debug, Clone)]
pub struct FileViewpoint {
    pub guid: String,
    pub view: ViewpointRequest,
    /// PNG image of the view.
    pub snapshot: Option<Vec<u8>>,
}

/// Escapes text for XML content and attributes.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\r' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

fn element(out: &mut String, indent: &str, name: &str, value: &str) {
    if !value.is_empty() {
        out.push_str(&format!("{indent}<{name}>{}</{name}>\n", xml_escape(value)));
    }
}

fn markup(topic: &FileTopic) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Markup>\n");
    let mut attributes = format!(" Guid=\"{}\"", xml_escape(&topic.guid));
    if !topic.topic_type.is_empty() {
        attributes.push_str(&format!(" TopicType=\"{}\"", xml_escape(&topic.topic_type)));
    }
    if !topic.topic_status.is_empty() {
        attributes.push_str(&format!(
            " TopicStatus=\"{}\"",
            xml_escape(&topic.topic_status)
        ));
    }
    x.push_str(&format!("  <Topic{attributes}>\n"));
    // Element order follows the BCF 2.1 schema.
    let i = "    ";
    element(&mut x, i, "Title", &topic.title);
    element(&mut x, i, "Priority", &topic.priority);
    if let Some(index) = topic.index {
        element(&mut x, i, "Index", &index.to_string());
    }
    for label in &topic.labels {
        element(&mut x, i, "Labels", label);
    }
    element(&mut x, i, "CreationDate", &topic.creation_date);
    element(&mut x, i, "CreationAuthor", &topic.creation_author);
    element(&mut x, i, "ModifiedDate", &topic.modified_date);
    element(&mut x, i, "AssignedTo", &topic.assigned_to);
    element(&mut x, i, "Description", &topic.description);
    x.push_str("  </Topic>\n");
    if let Some(vp) = &topic.viewpoint {
        x.push_str(&format!(
            "  <Viewpoints Guid=\"{}\">\n    <Viewpoint>viewpoint.bcfv</Viewpoint>\n",
            xml_escape(&vp.guid)
        ));
        if vp.snapshot.is_some() {
            x.push_str("    <Snapshot>snapshot.png</Snapshot>\n");
        }
        x.push_str("  </Viewpoints>\n");
    }
    x.push_str("</Markup>\n");
    x
}

fn point(out: &mut String, indent: &str, name: &str, p: &crate::bcf::Point) {
    out.push_str(&format!(
        "{indent}<{name}>\n{indent}  <X>{}</X>\n{indent}  <Y>{}</Y>\n{indent}  <Z>{}</Z>\n{indent}</{name}>\n",
        p.x, p.y, p.z
    ));
}

fn viewpoint(vp: &FileViewpoint) -> String {
    let v = &vp.view;
    let mut x = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VisualizationInfo Guid=\"{}\">\n  <Components>\n",
        xml_escape(&vp.guid)
    );
    let component = |guid: &str| format!("      <Component IfcGuid=\"{}\" />\n", xml_escape(guid));
    if !v.components.selection.is_empty() {
        x.push_str("    <Selection>\n");
        for c in &v.components.selection {
            x.push_str(&component(&c.ifc_guid));
        }
        x.push_str("    </Selection>\n");
    }
    x.push_str(&format!(
        "    <Visibility DefaultVisibility=\"{}\">\n",
        v.components.visibility.default_visibility
    ));
    if !v.components.visibility.exceptions.is_empty() {
        x.push_str("      <Exceptions>\n");
        for c in &v.components.visibility.exceptions {
            x.push_str(&format!("  {}", component(&c.ifc_guid)));
        }
        x.push_str("      </Exceptions>\n");
    }
    x.push_str("    </Visibility>\n  </Components>\n  <PerspectiveCamera>\n");
    let camera = &v.perspective_camera;
    point(&mut x, "    ", "CameraViewPoint", &camera.camera_view_point);
    point(&mut x, "    ", "CameraDirection", &camera.camera_direction);
    point(&mut x, "    ", "CameraUpVector", &camera.camera_up_vector);
    x.push_str(&format!(
        "    <FieldOfView>{}</FieldOfView>\n  </PerspectiveCamera>\n",
        camera.field_of_view
    ));
    if !v.clipping_planes.is_empty() {
        x.push_str("  <ClippingPlanes>\n");
        for plane in &v.clipping_planes {
            x.push_str("    <ClippingPlane>\n");
            point(&mut x, "      ", "Location", &plane.location);
            point(&mut x, "      ", "Direction", &plane.direction);
            x.push_str("    </ClippingPlane>\n");
        }
        x.push_str("  </ClippingPlanes>\n");
    }
    x.push_str("</VisualizationInfo>\n");
    x
}

/// Writes the topics as a BCF 2.1 zip archive.
pub fn write_bcfzip(project_name: &str, topics: &[FileTopic]) -> std::io::Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("bcf.version", options)?;
    zip.write_all(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Version VersionId=\"2.1\">\n  <DetailedVersion>2.1</DetailedVersion>\n</Version>\n",
    )?;
    if !project_name.is_empty() {
        zip.start_file("project.bcfp", options)?;
        let id = crate::project::sha256_hex(project_name.as_bytes());
        let id = format!(
            "{}-{}-{}-{}-{}",
            &id[0..8],
            &id[8..12],
            &id[12..16],
            &id[16..20],
            &id[20..32]
        );
        zip.write_all(
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ProjectExtension>\n  <Project ProjectId=\"{id}\">\n    <Name>{}</Name>\n  </Project>\n  <ExtensionSchema></ExtensionSchema>\n</ProjectExtension>\n",
                xml_escape(project_name)
            )
            .as_bytes(),
        )?;
    }
    for topic in topics {
        let dir = &topic.guid;
        zip.start_file(format!("{dir}/markup.bcf"), options)?;
        zip.write_all(markup(topic).as_bytes())?;
        if let Some(vp) = &topic.viewpoint {
            zip.start_file(format!("{dir}/viewpoint.bcfv"), options)?;
            zip.write_all(viewpoint(vp).as_bytes())?;
            if let Some(png) = &vp.snapshot {
                zip.start_file(format!("{dir}/snapshot.png"), stored)?;
                zip.write_all(png)?;
            }
        }
    }
    Ok(zip.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bcf::{ComponentRef, Components, Visibility, perspective_camera};
    use glam::DVec3;
    use std::io::Read;

    #[test]
    fn writes_a_readable_bcf_archive() {
        let topic = FileTopic {
            guid: "0b6e2f0a-1111-4222-8333-444455556666".into(),
            title: "Duct <vs> beam & co".into(),
            topic_type: "K&K-kontrol".into(),
            topic_status: "Open".into(),
            priority: "C3".into(),
            index: Some(1),
            labels: vec!["Ventilation".into(), "Konstruktion".into()],
            creation_date: "2026-10-20T10:00:00Z".into(),
            creation_author: "ABC".into(),
            assigned_to: "ABC".into(),
            description: "Line 1\nLine 2".into(),
            viewpoint: Some(FileViewpoint {
                guid: "9a6e2f0a-1111-4222-8333-444455556666".into(),
                view: ViewpointRequest {
                    perspective_camera: perspective_camera(
                        DVec3::new(1.0, 2.0, 3.0),
                        0.0,
                        0.0,
                        1.0,
                    ),
                    components: Components {
                        selection: vec![ComponentRef {
                            ifc_guid: "2O2Fr$t4X7Zf8NOew3FLOH".into(),
                        }],
                        visibility: Visibility {
                            default_visibility: true,
                            exceptions: vec![],
                        },
                    },
                    clipping_planes: vec![],
                    snapshot: None,
                },
                snapshot: Some(vec![0x89, b'P', b'N', b'G']),
            }),
            ..Default::default()
        };
        let bytes = write_bcfzip("Example", &[topic]).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "0b6e2f0a-1111-4222-8333-444455556666/markup.bcf",
                "0b6e2f0a-1111-4222-8333-444455556666/snapshot.png",
                "0b6e2f0a-1111-4222-8333-444455556666/viewpoint.bcfv",
                "bcf.version",
                "project.bcfp",
            ]
        );
        let mut markup = String::new();
        zip.by_name("0b6e2f0a-1111-4222-8333-444455556666/markup.bcf")
            .unwrap()
            .read_to_string(&mut markup)
            .unwrap();
        assert!(markup.contains("<Title>Duct &lt;vs&gt; beam &amp; co</Title>"));
        assert!(markup.contains("TopicType=\"K&amp;K-kontrol\""));
        assert!(markup.contains("<Priority>C3</Priority>"));
        assert!(markup.contains("<Snapshot>snapshot.png</Snapshot>"));
        let mut vp = String::new();
        zip.by_name("0b6e2f0a-1111-4222-8333-444455556666/viewpoint.bcfv")
            .unwrap()
            .read_to_string(&mut vp)
            .unwrap();
        assert!(vp.contains("<Component IfcGuid=\"2O2Fr$t4X7Zf8NOew3FLOH\" />"));
        assert!(vp.contains("<CameraViewPoint>"));
        assert!(vp.contains("DefaultVisibility=\"true\""));
    }
}
