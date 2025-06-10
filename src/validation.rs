use crate::{
    error::{Result, ShapleyError},
    types::{DemandMatrix, Link, PrivateLinks, PublicLinks},
};
use std::collections::HashSet;

/// Validate that private links have all required fields
pub fn validate_private_links(links: &PrivateLinks) -> Result<()> {
    if links.is_empty() {
        return Err(ShapleyError::EmptyLinks {
            link_type: "private".to_string(),
        });
    }

    // In our type system, all required fields are guaranteed to exist
    // This validation is more about the structure being correct
    Ok(())
}

/// Validate that public links have required structure
pub fn validate_public_links(links: &PublicLinks) -> Result<()> {
    if links.is_empty() {
        return Err(ShapleyError::EmptyLinks {
            link_type: "public".to_string(),
        });
    }

    Ok(())
}

/// Validate that switches are properly labeled with integers
pub fn validate_switch_naming(links: &[Link], link_type: &str) -> Result<()> {
    for link in links {
        if !has_digit(&link.start) || !has_digit(&link.end) {
            return Err(ShapleyError::InvalidSwitchNaming {
                link_type: link_type.to_string(),
            });
        }
    }
    Ok(())
}

/// Validate that endpoints in demand matrix don't have integers
pub fn validate_endpoint_naming(demand: &DemandMatrix) -> Result<()> {
    for d in &demand.demands {
        if has_digit(&d.start) || has_digit(&d.end) {
            return Err(ShapleyError::InvalidEndpointNaming);
        }
    }
    Ok(())
}

/// Validate that each traffic type has a single source
pub fn validate_traffic_types(demand: &DemandMatrix) -> Result<()> {
    let mut type_sources: std::collections::HashMap<usize, HashSet<String>> =
        std::collections::HashMap::new();

    for d in &demand.demands {
        type_sources
            .entry(d.demand_type)
            .or_default()
            .insert(d.start.clone());
    }

    for (_, sources) in type_sources {
        if sources.len() > 1 {
            return Err(ShapleyError::MultipleTrafficSources);
        }
    }

    Ok(())
}

/// Validate that public pathways cover all switches and demand nodes
pub fn validate_public_pathway_coverage(
    private_links: &[Link],
    public_links: &[Link],
    demand: &DemandMatrix,
) -> Result<()> {
    // Check switch coverage
    let mut private_switches = HashSet::new();
    for link in private_links {
        private_switches.insert(link.start.clone());
        private_switches.insert(link.end.clone());
    }

    let mut public_switches = HashSet::new();
    for link in public_links {
        public_switches.insert(link.start.clone());
        public_switches.insert(link.end.clone());
    }

    let missing_switches: Vec<String> = private_switches
        .difference(&public_switches)
        .cloned()
        .collect();

    if !missing_switches.is_empty() {
        return Err(ShapleyError::IncompletePublicPathway {
            location: "all the switches".to_string(),
            missing: missing_switches,
        });
    }

    // Check demand node coverage
    let mut demand_cities = HashSet::new();
    for d in &demand.demands {
        demand_cities.insert(d.start.clone());
        demand_cities.insert(d.end.clone());
    }

    let mut public_cities = HashSet::new();
    for switch in &public_switches {
        if switch.len() >= 3 {
            public_cities.insert(switch[..3].to_string());
        }
    }

    let missing_cities: Vec<String> = demand_cities.difference(&public_cities).cloned().collect();

    if !missing_cities.is_empty() {
        return Err(ShapleyError::IncompletePublicPathway {
            location: "the demand points".to_string(),
            missing: missing_cities,
        });
    }

    Ok(())
}

/// Validate operator names don't use reserved keywords
pub fn validate_operator_names(operators: &[String]) -> Result<()> {
    // Remove duplicates and check
    let unique_ops: HashSet<&String> = operators.iter().collect();

    // Check if any operator is "0"
    if operators.iter().any(|op| op == "0") {
        return Err(ShapleyError::ReservedOperatorName);
    }

    if unique_ops.len() > 15 {
        return Err(ShapleyError::TooManyOperators);
    }

    Ok(())
}

/// Check if a string contains a digit
#[inline]
fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DemandBuilder, LinkBuilder};
    use rust_decimal::prelude::*;

    #[test]
    fn test_validate_empty_links() {
        let empty_private = PrivateLinks::from_links(vec![]);
        assert!(matches!(
            validate_private_links(&empty_private),
            Err(ShapleyError::EmptyLinks { .. })
        ));

        let empty_public = PublicLinks::from_links(vec![]);
        assert!(matches!(
            validate_public_links(&empty_public),
            Err(ShapleyError::EmptyLinks { .. })
        ));
    }

    #[test]
    fn test_validate_switch_naming() {
        // Valid switches (with digits)
        let mut link = LinkBuilder::default()
            .start("NYC1".to_string())
            .end("LAX1".to_string())
            .build()
            .unwrap();
        let links = vec![link.clone()];
        assert!(validate_switch_naming(&links, "private").is_ok());

        // Invalid switches (no digits)
        link.start = "NYC".to_string();
        link.end = "LAX".to_string();
        let links = vec![link.clone()];
        assert!(matches!(
            validate_switch_naming(&links, "private"),
            Err(ShapleyError::InvalidSwitchNaming { .. })
        ));

        // Mixed - still invalid
        link.start = "NYC1".to_string();
        link.end = "LAX".to_string();
        let links = vec![link];
        assert!(matches!(
            validate_switch_naming(&links, "public"),
            Err(ShapleyError::InvalidSwitchNaming { .. })
        ));
    }

    #[test]
    fn test_validate_endpoint_naming() {
        // Valid endpoints (no digits)
        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);
        assert!(validate_endpoint_naming(&matrix).is_ok());

        // Invalid endpoints (with digits)
        let demands = vec![
            DemandBuilder::default()
                .start("NYC1".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);
        assert!(matches!(
            validate_endpoint_naming(&matrix),
            Err(ShapleyError::InvalidEndpointNaming)
        ));
    }

    #[test]
    fn test_validate_traffic_types() {
        // Valid - single source per type
        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("CHI".to_string())
                .traffic(Decimal::from(20))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("LAX".to_string())
                .end("CHI".to_string())
                .traffic(Decimal::from(30))
                .demand_type(2)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);
        assert!(validate_traffic_types(&matrix).is_ok());

        // Invalid - multiple sources for type 1
        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("CHI".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(20))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);
        assert!(matches!(
            validate_traffic_types(&matrix),
            Err(ShapleyError::MultipleTrafficSources)
        ));
    }

    #[test]
    fn test_validate_public_pathway_coverage() {
        // Setup private links
        let private_link = LinkBuilder::default()
            .start("NYC1".to_string())
            .end("LAX1".to_string())
            .operator1("Op1".to_string())
            .build()
            .unwrap();
        let private_links = vec![private_link];

        // Valid public coverage
        let public_links = vec![
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("LAX1".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("LAX1".to_string())
                .end("NYC1".to_string())
                .build()
                .unwrap(),
        ];

        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("LAX".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);

        assert!(validate_public_pathway_coverage(&private_links, &public_links, &matrix).is_ok());

        // Missing switch coverage
        let public_links = vec![
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("CHI1".to_string())
                .build()
                .unwrap(),
        ];
        assert!(matches!(
            validate_public_pathway_coverage(&private_links, &public_links, &matrix),
            Err(ShapleyError::IncompletePublicPathway { .. })
        ));

        // Missing demand coverage
        let public_links = vec![
            LinkBuilder::default()
                .start("NYC1".to_string())
                .end("LAX1".to_string())
                .build()
                .unwrap(),
            LinkBuilder::default()
                .start("LAX1".to_string())
                .end("NYC1".to_string())
                .build()
                .unwrap(),
        ];
        let demands = vec![
            DemandBuilder::default()
                .start("NYC".to_string())
                .end("CHI".to_string())
                .traffic(Decimal::from(10))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        let matrix = DemandMatrix::from_demands(demands);
        assert!(matches!(
            validate_public_pathway_coverage(&private_links, &public_links, &matrix),
            Err(ShapleyError::IncompletePublicPathway { .. })
        ));
    }

    #[test]
    fn test_validate_operator_names() {
        // Valid operators
        let operators = vec!["Alpha".to_string(), "Beta".to_string(), "Gamma".to_string()];
        assert!(validate_operator_names(&operators).is_ok());

        // Reserved operator name
        let operators = vec!["Alpha".to_string(), "0".to_string()];
        assert!(matches!(
            validate_operator_names(&operators),
            Err(ShapleyError::ReservedOperatorName)
        ));

        // Too many operators
        let operators: Vec<String> = (0..16).map(|i| format!("Op{}", i)).collect();
        assert!(matches!(
            validate_operator_names(&operators),
            Err(ShapleyError::TooManyOperators)
        ));
    }

    #[test]
    fn test_has_digit() {
        assert!(has_digit("NYC1"));
        assert!(has_digit("123"));
        assert!(has_digit("LAX2CHI"));
        assert!(!has_digit("NYC"));
        assert!(!has_digit(""));
        assert!(!has_digit("ABC"));
    }
}
