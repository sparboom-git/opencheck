//! BCF API 2.1 data types (buildingSMART BCF-API), as sent to and received
//! from issue servers such as Dalux. Only data and pure helpers live here;
//! the HTTP client is in the application.

use glam::DVec3;
use serde::{Deserialize, Deserializer, Serialize};

/// The BCF API version OpenCheck speaks.
pub const API_VERSION: &str = "2.1";

/// Reads `null` as the empty value. Servers such as Dalux send `null` instead
/// of an empty list or text, which serde would otherwise refuse.
pub fn null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// `GET /bcf/versions`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Versions {
    #[serde(default, deserialize_with = "null_as_default")]
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub version_id: String,
}

/// `GET /bcf/{version}/auth`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuthInfo {
    #[serde(default)]
    pub oauth2_auth_url: Option<String>,
    #[serde(default)]
    pub oauth2_token_url: Option<String>,
    #[serde(default)]
    pub http_basic_supported: bool,
    #[serde(default, deserialize_with = "null_as_default")]
    pub supported_oauth2_flows: Vec<String>,
}

/// OAuth2 token response.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires.
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// `GET /bcf/{version}/current-user`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CurrentUser {
    #[serde(default, deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub name: String,
}

/// `GET /bcf/{version}/projects`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub project_id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub name: String,
    /// What the signed-in user may do in the project, e.g. `createTopic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<ProjectAuthorization>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectAuthorization {
    #[serde(default, deserialize_with = "null_as_default")]
    pub project_actions: Vec<String>,
}

impl Project {
    /// False only when the server lists the user's rights and creating
    /// topics is not among them.
    pub fn may_create_topics(&self) -> bool {
        self.authorization.as_ref().is_none_or(|a| {
            a.project_actions.is_empty()
                || a.project_actions
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case("createTopic"))
        })
    }
}

/// `GET /bcf/{version}/projects/{id}/extensions`: the values the server
/// accepts. `user_id_type` lists the people topics can be assigned to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extensions {
    #[serde(default, deserialize_with = "null_as_default")]
    pub topic_type: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub topic_status: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub topic_label: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub snippet_type: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub priority: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub user_id_type: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub stage: Vec<String>,
    /// What the user may do in the project (`createTopic`, `update`, ...).
    /// BCF API 2.1 lists the rights here; empty when the server does not say.
    #[serde(default, deserialize_with = "null_as_default")]
    pub project_actions: Vec<String>,
    /// What the user may do with topics (`update`, `createComment`, ...).
    #[serde(default, deserialize_with = "null_as_default")]
    pub topic_actions: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub comment_actions: Vec<String>,
}

impl Extensions {
    /// False only when the server lists the rights and `createTopic` is not
    /// among them.
    pub fn may_create_topics(&self) -> bool {
        self.project_actions.is_empty()
            || self
                .project_actions
                .iter()
                .any(|a| a.eq_ignore_ascii_case("createTopic"))
    }
}

/// Body of `POST`/`PUT .../topics`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TopicRequest {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

/// A topic as the server returns it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Topic {
    #[serde(default, deserialize_with = "null_as_default")]
    pub guid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub title: String,
    pub topic_type: Option<String>,
    pub topic_status: Option<String>,
    pub priority: Option<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    pub labels: Vec<String>,
    pub assigned_to: Option<String>,
    pub description: Option<String>,
    pub creation_date: Option<String>,
    pub modified_date: Option<String>,
}

/// Body of `POST .../topics/{guid}/comments`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommentRequest {
    pub comment: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Comment {
    #[serde(default, deserialize_with = "null_as_default")]
    pub guid: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub date: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub author: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub comment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl From<DVec3> for Point {
    fn from(v: DVec3) -> Self {
        Point {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerspectiveCamera {
    pub camera_view_point: Point,
    pub camera_direction: Point,
    pub camera_up_vector: Point,
    /// Vertical field of view in degrees.
    pub field_of_view: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComponentRef {
    pub ifc_guid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Visibility {
    pub default_visibility: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<ComponentRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Components {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<ComponentRef>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClippingPlane {
    pub location: Point,
    /// Points to the side that is cut away.
    pub direction: Point,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub snapshot_type: String,
    /// Base64-encoded image.
    pub snapshot_data: String,
}

/// Body of `POST .../topics/{guid}/viewpoints`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ViewpointRequest {
    pub perspective_camera: PerspectiveCamera,
    pub components: Components,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clipping_planes: Vec<ClippingPlane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
}

/// The BCF API root for a server address as users get it, e.g.
/// `https://node1.field.dalux.com/service/` gives
/// `https://node1.field.dalux.com/service/bcf/2.1`.
pub fn api_root(server: &str) -> String {
    let base = server.trim().trim_end_matches('/');
    let base = base
        .strip_suffix(&format!("/bcf/{API_VERSION}"))
        .or_else(|| base.strip_suffix("/bcf"))
        .unwrap_or(base);
    format!("{base}/bcf/{API_VERSION}")
}

/// The first of `candidates` that the server offers, compared without case.
/// Servers name statuses and priorities differently ("Open", "Åben", ...).
pub fn pick<'a>(offered: &'a [String], candidates: &[&str]) -> Option<&'a String> {
    candidates
        .iter()
        .find_map(|c| offered.iter().find(|o| same_name(o, c)))
}

/// Names compared without case, also for letters such as Æ, Ø and Å.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A BCF camera from OpenCheck's eye position, heading and pitch (radians,
/// heading measured from +Y towards +X) and vertical field of view.
pub fn perspective_camera(eye: DVec3, yaw: f64, pitch: f64, fov_y: f64) -> PerspectiveCamera {
    let direction = DVec3::new(
        yaw.sin() * pitch.cos(),
        yaw.cos() * pitch.cos(),
        pitch.sin(),
    );
    let right = direction.cross(DVec3::Z).normalize_or(DVec3::X);
    let up = right.cross(direction).normalize_or(DVec3::Z);
    PerspectiveCamera {
        camera_view_point: eye.into(),
        camera_direction: direction.into(),
        camera_up_vector: up.into(),
        field_of_view: fov_y.to_degrees(),
    }
}

/// URL-safe Base64 without padding (for PKCE).
pub fn base64_url(bytes: &[u8]) -> String {
    base64_with(
        bytes,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
        false,
    )
}

/// Standard Base64 with padding (for snapshots).
pub fn base64(bytes: &[u8]) -> String {
    base64_with(
        bytes,
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
        true,
    )
}

fn base64_with(bytes: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= n {
                out.push(alphabet[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

/// `application/x-www-form-urlencoded` / query-string escaping.
pub fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodes `%XX` escapes and `+` in a query value.
pub fn url_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_root_from_the_address_users_get() {
        let want = "https://node1.field.dalux.com/service/bcf/2.1";
        assert_eq!(api_root("https://node1.field.dalux.com/service/"), want);
        assert_eq!(api_root("https://node1.field.dalux.com/service"), want);
        assert_eq!(
            api_root("https://node1.field.dalux.com/service/bcf/2.1/"),
            want
        );
    }

    #[test]
    fn picks_offered_values_without_case() {
        let offered = vec!["Åben".to_owned(), "Lukket".to_owned()];
        assert_eq!(pick(&offered, &["Open", "åben"]), Some(&offered[0]));
        assert_eq!(pick(&offered, &["Closed"]), None);
    }

    #[test]
    fn camera_looks_along_heading_with_upright_up_vector() {
        let c = perspective_camera(DVec3::ZERO, 0.0, 0.0, 45f64.to_radians());
        assert!((c.camera_direction.y - 1.0).abs() < 1e-9);
        assert!((c.camera_up_vector.z - 1.0).abs() < 1e-9);
        assert!((c.field_of_view - 45.0).abs() < 1e-9);
        let down = perspective_camera(DVec3::ZERO, 0.0, -0.5, 1.0);
        assert!(down.camera_direction.z < 0.0 && down.camera_up_vector.z > 0.0);
    }

    #[test]
    fn base64_and_url_escaping() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
        assert_eq!(url_encode("a b&c=ø"), "a%20b%26c%3D%C3%B8");
        assert_eq!(url_decode("a%20b+c%3D%C3%B8"), "a b c=ø");
    }

    #[test]
    fn topics_and_extensions_parse_with_missing_fields() {
        let t: Topic =
            serde_json::from_str(r#"{"guid":"g","title":"T","topic_status":"Open"}"#).unwrap();
        assert_eq!(t.topic_status.as_deref(), Some("Open"));
        // As Dalux answers: rights in the extensions, nulls for empty lists.
        let dalux: Extensions = serde_json::from_str(
            r#"{"topic_type":["BCF"],"topic_label":null,"project_actions":["update"],
                "topic_actions":["update","createComment"],"comment_actions":[]}"#,
        )
        .unwrap();
        assert!(!dalux.may_create_topics());
        assert!(Extensions::default().may_create_topics());
        let e: Extensions = serde_json::from_str(r#"{"user_id_type":["a@b.dk"]}"#).unwrap();
        assert_eq!(e.user_id_type, ["a@b.dk"]);
    }

    #[test]
    fn null_lists_and_texts_parse_as_empty() {
        // Dalux answers `null` for lists it has no values for.
        let e: Extensions = serde_json::from_str(
            r#"{"topic_type":["Fejl"],"topic_status":null,"topic_label":null,
                "snippet_type":null,"priority":null,"user_id_type":null,"stage":null,
                "project_actions":null}"#,
        )
        .unwrap();
        assert_eq!(e.topic_type, ["Fejl"]);
        assert!(e.topic_status.is_empty() && e.user_id_type.is_empty() && e.stage.is_empty());
        let t: Topic =
            serde_json::from_str(r#"{"guid":"g","title":null,"labels":null,"priority":null}"#)
                .unwrap();
        assert!(t.title.is_empty() && t.labels.is_empty() && t.priority.is_none());
        let c: Comment =
            serde_json::from_str(r#"{"guid":"c","author":null,"comment":null}"#).unwrap();
        assert!(c.author.is_empty());
        let u: CurrentUser = serde_json::from_str(r#"{"id":"u","name":null}"#).unwrap();
        assert_eq!(u.id, "u");
        let p: Project = serde_json::from_str(r#"{"project_id":"p","name":null}"#).unwrap();
        assert!(p.name.is_empty());
        let a: AuthInfo = serde_json::from_str(r#"{"supported_oauth2_flows":null}"#).unwrap();
        assert!(a.supported_oauth2_flows.is_empty());
    }
}
