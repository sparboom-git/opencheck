use std::fmt;

/// The design discipline a model belongs to.
///
/// Rules filter on discipline (for example "MEP against Structural"), so every
/// loaded model gets one. It is guessed on load and can be changed by the user.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Discipline {
    Architectural,
    Structural,
    Mechanical,
    Plumbing,
    Electrical,
    FireProtection,
    Landscape,
    Infrastructure,
    #[default]
    Unknown,
}

impl Discipline {
    pub const ALL: [Discipline; 9] = [
        Discipline::Architectural,
        Discipline::Structural,
        Discipline::Mechanical,
        Discipline::Plumbing,
        Discipline::Electrical,
        Discipline::FireProtection,
        Discipline::Landscape,
        Discipline::Infrastructure,
        Discipline::Unknown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Discipline::Architectural => "Architectural",
            Discipline::Structural => "Structural",
            Discipline::Mechanical => "Ventilation",
            Discipline::Plumbing => "Plumbing",
            Discipline::Electrical => "Electrical",
            Discipline::FireProtection => "Sprinkler",
            Discipline::Landscape => "Landscape",
            Discipline::Infrastructure => "Infrastructure",
            Discipline::Unknown => "Unknown",
        }
    }

    /// Short code used in compact UI and in rule files.
    pub fn code(self) -> &'static str {
        match self {
            Discipline::Architectural => "ARC",
            Discipline::Structural => "STR",
            Discipline::Mechanical => "VEN",
            Discipline::Plumbing => "PLU",
            Discipline::Electrical => "ELE",
            Discipline::FireProtection => "SPR",
            Discipline::Landscape => "LAN",
            Discipline::Infrastructure => "INF",
            Discipline::Unknown => "UNK",
        }
    }

    /// Guesses the discipline from the file name and the element classes in the model.
    ///
    /// File names win because offices name their files by discipline
    /// (`K_Building.ifc`, `ARK-model.ifc`, `Building-Hvac.ifc`). When the name gives
    /// nothing away, the most common discipline-specific element class decides.
    pub fn guess<'a>(file_name: &str, ifc_classes: impl IntoIterator<Item = &'a str>) -> Self {
        if let Some(d) = Self::guess_from_file_name(file_name) {
            return d;
        }
        Self::guess_from_classes(ifc_classes)
    }

    /// The discipline of a Danish K code, as in "K09" (structure). Codes
    /// that are no discipline of a model check (K00 shared, K05 design,
    /// K06 survey, K10 process plants) give none.
    pub fn from_k_code(token: &str) -> Option<Self> {
        let digits = token
            .strip_prefix('k')
            .or_else(|| token.strip_prefix('K'))?;
        if digits.len() != 2 || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(match digits {
            // Architecture and interiors.
            "01" | "03" => Discipline::Architectural,
            // Landscape and terrain, urban spaces.
            "02" | "04" => Discipline::Landscape,
            // Electrical installations and IT.
            "07" => Discipline::Electrical,
            // Mechanical installations: ventilation, unless the name says
            // water or drainage (see `guess_from_file_name`).
            "08" => Discipline::Mechanical,
            "09" => Discipline::Structural,
            // Transport and infrastructure.
            "11" => Discipline::Infrastructure,
            // Shared, design, survey and process plants are no discipline
            // here: the rest of the name decides.
            _ => return None,
        })
    }

    fn guess_from_file_name(file_name: &str) -> Option<Self> {
        let stem = file_name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(file_name)
            .to_ascii_lowercase();
        // Danish file names often prefix the discipline code with a Z, as in
        // "P0001_K07_ZEL-model": both "zel" and "el" count as tokens.
        let tokens: Vec<&str> = stem
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
            .flat_map(|t| {
                let without_z = t.strip_prefix('z').filter(|r| r.len() >= 2);
                std::iter::once(t).chain(without_z)
            })
            .collect();
        // A K code (the Danish discipline code in file names, e.g. "K09")
        // decides before anything else.
        let has = |words: &[&str]| tokens.iter().any(|t| words.contains(t));
        if let Some(d) = tokens.iter().find_map(|t| Self::from_k_code(t)) {
            // K08 covers water and drainage too.
            if d == Discipline::Mechanical
                && (has(&["vvs", "plu", "san", "vand", "afl"])
                    || stem.contains("plumbing")
                    || stem.contains("sanitary"))
            {
                return Some(Discipline::Plumbing);
            }
            return Some(d);
        }
        let contains = |words: &[&str]| words.iter().any(|w| stem.contains(w));

        // Order matters: "hvac" must be checked before shorter tokens such as "el".
        if contains(&["hvac", "mechanical", "ventilation"])
            || has(&["mep", "vent", "ven", "mec", "ventilation"])
        {
            return Some(Discipline::Mechanical);
        }
        if has(&["spr", "sprinkler", "bra", "brand"]) {
            return Some(Discipline::FireProtection);
        }
        if contains(&["plumbing", "sanitary", "sprinkler"])
            || has(&["vvs", "plu", "san", "vand", "afl"])
        {
            return Some(Discipline::Plumbing);
        }
        if contains(&["electrical"]) || has(&["el", "ele", "elec"]) {
            return Some(Discipline::Electrical);
        }
        if contains(&["structur", "konstruktion"]) || has(&["str", "kon", "k", "rib"]) {
            return Some(Discipline::Structural);
        }
        if contains(&["architect", "arkitekt"]) || has(&["arc", "ark", "arch", "a"]) {
            return Some(Discipline::Architectural);
        }
        if contains(&["landscap", "landskab"]) || has(&["lan", "land"]) {
            return Some(Discipline::Landscape);
        }
        if contains(&["infra", "road", "rail", "bridge"]) {
            return Some(Discipline::Infrastructure);
        }
        None
    }

    fn guess_from_classes<'a>(ifc_classes: impl IntoIterator<Item = &'a str>) -> Self {
        let mut votes = [0usize; 9];
        for class in ifc_classes {
            let d = match class {
                "IfcBeam" | "IfcColumn" | "IfcFooting" | "IfcPile" | "IfcReinforcingBar"
                | "IfcReinforcingMesh" | "IfcTendon" => Discipline::Structural,
                "IfcDuctSegment"
                | "IfcDuctFitting"
                | "IfcAirTerminal"
                | "IfcFan"
                | "IfcUnitaryEquipment"
                | "IfcDamper" => Discipline::Mechanical,
                "IfcPipeSegment"
                | "IfcPipeFitting"
                | "IfcSanitaryTerminal"
                | "IfcValve"
                | "IfcPump"
                | "IfcWasteTerminal" => Discipline::Plumbing,
                "IfcCableSegment"
                | "IfcCableCarrierSegment"
                | "IfcCableFitting"
                | "IfcLightFixture"
                | "IfcOutlet"
                | "IfcSwitchingDevice" => Discipline::Electrical,
                "IfcFireSuppressionTerminal" | "IfcAlarm" => Discipline::FireProtection,
                "IfcDoor" | "IfcWindow" | "IfcCovering" | "IfcCurtainWall" | "IfcRailing"
                | "IfcStair" | "IfcSpace" => Discipline::Architectural,
                "IfcGeographicElement" => Discipline::Landscape,
                "IfcRoad" | "IfcRailway" | "IfcBridge" | "IfcAlignment" => {
                    Discipline::Infrastructure
                }
                _ => continue,
            };
            votes[d as usize] += 1;
        }
        let (best, count) = votes
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .expect("votes is non-empty");
        if *count == 0 {
            Discipline::Unknown
        } else {
            Discipline::ALL[best]
        }
    }
}

impl fmt::Display for Discipline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_from_file_name() {
        for (name, expected) in [
            ("P0001_C07.02_K07_ZEL-model.ifc", Discipline::Electrical),
            ("P0001_C07.02_K09_ZKON-model.ifc", Discipline::Structural),
            ("P0001_C07.02_K01_ZARK-model.ifc", Discipline::Architectural),
            ("P0001_C07.02_K08_ZVENT-model.ifc", Discipline::Mechanical),
            ("Projekt_ZVVS.ifc", Discipline::Plumbing),
            // The K code decides, whatever else the name says.
            ("Newtec_K07_GEN_F3_N001.ifc", Discipline::Electrical),
            ("Newtec_K08_GEN_F3_N001.ifc", Discipline::Mechanical),
            ("NEWTEC_K01_ADM_F3_N001.ifc", Discipline::Architectural),
            ("Newtec_K09_GEN_F3_N001.ifc", Discipline::Structural),
            ("P1_K08_ZVVS.ifc", Discipline::Plumbing),
            ("P1_K03_Indretning.ifc", Discipline::Architectural),
            ("P1_K11_GEN.ifc", Discipline::Infrastructure),
            // Codes that are no discipline leave it to the rest of the name.
            ("P1_K00_KON.ifc", Discipline::Structural),
        ] {
            assert_eq!(Discipline::guess(name, []), expected, "{name}");
        }
        assert_eq!(
            Discipline::guess("Building-Hvac.ifc", []),
            Discipline::Mechanical
        );
        assert_eq!(
            Discipline::guess("Building-Structural.ifc", []),
            Discipline::Structural
        );
        assert_eq!(
            Discipline::guess("C:\\proj\\K_Hus.ifc", []),
            Discipline::Structural
        );
        assert_eq!(
            Discipline::guess("ARK_model.ifc", []),
            Discipline::Architectural
        );
        assert_eq!(Discipline::guess("VVS-stue.ifc", []), Discipline::Plumbing);
        assert_eq!(
            Discipline::guess("Projekt_EL.ifc", []),
            Discipline::Electrical
        );
    }

    #[test]
    fn falls_back_to_element_classes() {
        let classes = ["IfcBeam", "IfcBeam", "IfcColumn", "IfcDoor"];
        assert_eq!(
            Discipline::guess("model.ifc", classes),
            Discipline::Structural
        );
        assert_eq!(Discipline::guess("model.ifc", []), Discipline::Unknown);
    }
}
