//! Naming utilities for code generation.
//!
//! Centralized string transformation functions used across all codegen layers.

/// Convert `CamelCase` to `snake_case`.
pub fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }
    result
}

/// Convert `snake_case` to `PascalCase`.
pub fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().collect::<String>();
                    s.push_str(chars.as_str());
                    s
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Naive English pluralization for entity names.
pub fn pluralize(s: &str) -> String {
    if s.ends_with('s') || s.ends_with("sh") || s.ends_with("ch") || s.ends_with('x') {
        format!("{s}es")
    } else if s.ends_with('y') && !s.ends_with("ey") && !s.ends_with("ay") && !s.ends_with("oy") {
        format!("{}ies", &s[..s.len() - 1])
    } else {
        format!("{s}s")
    }
}

/// Derive the junction table name for a many-to-many relation.
///
/// Uses the explicit override from `RelationInfo.junction` if present,
/// otherwise defaults to `{entity_snake}_{field_name}`.
pub fn junction_table_name(entity_snake: &str, field_name: &str, junction_override: Option<&str>) -> String {
    junction_override.map(String::from).unwrap_or_else(|| format!("{entity_snake}_{field_name}"))
}

/// Derive the source FK column name for a junction table.
/// E.g., entity "Node" → "node_id".
pub fn junction_source_col(entity_snake: &str) -> String {
    format!("{entity_snake}_id")
}

/// Derive the target FK column name for a junction table.
/// E.g., target "Requirement" → "requirement_id".
/// Self-referential junctions use "target_id" to avoid ambiguity.
pub fn junction_target_col(_entity_snake: &str, target_snake: &str, is_self_ref: bool) -> String {
    if is_self_ref { "target_id".to_string() } else { format!("{target_snake}_id") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("Node"), "node");
        assert_eq!(to_snake_case("WorkSession"), "work_session");
        assert_eq!(to_snake_case("Agent"), "agent");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("parent_id"), "ParentId");
        assert_eq!(to_pascal_case("from"), "From");
        assert_eq!(to_pascal_case("work_item"), "WorkItem");
        assert_eq!(to_pascal_case("node"), "Node");
        assert_eq!(to_pascal_case("id"), "Id");
        assert_eq!(to_pascal_case("target_id"), "TargetId");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("node"), "nodes");
        assert_eq!(pluralize("agent"), "agents");
        assert_eq!(pluralize("role"), "roles");
        assert_eq!(pluralize("evidence"), "evidences");
        assert_eq!(pluralize("work_session"), "work_sessions");
        assert_eq!(pluralize("specification"), "specifications");
    }

    #[test]
    fn test_junction_table_name() {
        assert_eq!(junction_table_name("node", "fulfills", None), "node_fulfills");
        assert_eq!(junction_table_name("node", "fulfills", Some("custom_junction")), "custom_junction");
    }
}
