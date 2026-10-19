use std::fmt;

/// A property set (`IfcPropertySet`) or quantity set (`IfcElementQuantity`).
#[derive(Debug, Clone)]
pub struct PropertySet {
    pub express_id: u32,
    pub name: String,
    pub kind: PropertySetKind,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertySetKind {
    Properties,
    Quantities,
}

impl PropertySet {
    pub fn get(&self, name: &str) -> Option<&Property> {
        self.properties
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub value: PropertyValue,
    /// IFC measure type, e.g. `IfcLabel`, `IfcLengthMeasure` or `IfcQuantityArea`.
    pub value_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Text(String),
    Integer(i64),
    Real(f64),
    Boolean(bool),
    /// IFC LOGICAL: true, false or unknown (`None`).
    Logical(Option<bool>),
    List(Vec<PropertyValue>),
    Range {
        lower: Box<PropertyValue>,
        upper: Box<PropertyValue>,
    },
    Null,
}

impl PropertyValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PropertyValue::Real(v) => Some(*v),
            PropertyValue::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            PropertyValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for PropertyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PropertyValue::Text(s) => f.write_str(s),
            PropertyValue::Integer(v) => write!(f, "{v}"),
            PropertyValue::Real(v) => {
                // Trim float noise such as 2.4000000000000004 without hiding real precision.
                let s = format!("{v:.6}");
                let s = s.trim_end_matches('0').trim_end_matches('.');
                f.write_str(s)
            }
            PropertyValue::Boolean(b) => f.write_str(if *b { "True" } else { "False" }),
            PropertyValue::Logical(Some(b)) => f.write_str(if *b { "True" } else { "False" }),
            PropertyValue::Logical(None) => f.write_str("Unknown"),
            PropertyValue::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                Ok(())
            }
            PropertyValue::Range { lower, upper } => write!(f, "{lower} .. {upper}"),
            PropertyValue::Null => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_values_drop_float_noise() {
        assert_eq!(PropertyValue::Real(2.4000000000000004).to_string(), "2.4");
        assert_eq!(PropertyValue::Real(3.0).to_string(), "3");
        assert_eq!(PropertyValue::Real(0.125).to_string(), "0.125");
    }
}
