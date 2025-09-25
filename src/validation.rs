use crate::{
    error::{Result, ShapleyError},
    types::{Demands, Devices, PrivateLinks, PublicLinks},
    utils::has_digit,
};
use std::collections::HashSet;

/// Validate all inputs for network shapley computation
pub(crate) fn check_inputs(
    private_links: &PrivateLinks,
    devices: &Devices,
    demands: &Demands,
    public_links: &PublicLinks,
    operator_uptime: f64,
) -> Result<()> {
    // Check for "Public" operator name before filtering
    for device in devices {
        if device.operator == "Public" {
            return Err(ShapleyError::Validation(
                "Public is a protected keyword for operator names; choose another.".to_string(),
            ));
        }
    }

    // Check operator count (excluding "Private" and "Public")
    let operators: HashSet<&str> = devices
        .iter()
        .map(|d| d.operator.as_str())
        .filter(|&op| op != "Private" && op != "Public")
        .collect();

    let n_ops = operators.len();
    if operator_uptime < 1.0 {
        if n_ops >= 16 {
            return Err(ShapleyError::TooManyOperators {
                count: n_ops,
                limit: 15,
            });
        }
    } else if n_ops >= 21 {
        return Err(ShapleyError::TooManyOperators {
            count: n_ops,
            limit: 20,
        });
    }

    // Check that private links table is labeled correctly
    if private_links.is_empty() {
        return Err(ShapleyError::Validation(
            "There must be at least one private link for this simulation.".to_string(),
        ));
    }

    // Check that public links table is labeled correctly
    for link in public_links {
        if has_digit(&link.city1) {
            return Err(ShapleyError::InvalidCityLabel(format!(
                "City {} should not contain a digit",
                link.city1
            )));
        }
        if has_digit(&link.city2) {
            return Err(ShapleyError::InvalidCityLabel(format!(
                "City {} should not contain a digit",
                link.city2
            )));
        }
    }

    // Check that demand points are labeled correctly
    for demand in demands {
        if has_digit(&demand.start) {
            return Err(ShapleyError::InvalidCityLabel(format!(
                "City {} should not contain a digit",
                demand.start
            )));
        }
        if has_digit(&demand.end) {
            return Err(ShapleyError::InvalidCityLabel(format!(
                "City {} should not contain a digit",
                demand.end
            )));
        }
    }

    // Check that for a given demand type, there is a single origin, size, and multicast flag
    use std::collections::HashMap;
    let mut type_info: HashMap<u32, (&str, f64, bool)> = HashMap::new();

    for demand in demands {
        match type_info.get(&demand.kind) {
            Some(&(start, traffic, multicast)) => {
                if start != demand.start.as_str()
                    || traffic != demand.traffic
                    || multicast != demand.multicast
                {
                    return Err(ShapleyError::DataInconsistency(format!(
                        "Demand type {} has inconsistent properties",
                        demand.kind
                    )));
                }
            }
            None => {
                type_info.insert(
                    demand.kind,
                    (demand.start.as_str(), demand.traffic, demand.multicast),
                );
            }
        }
    }

    // Check there are no duplicate devices
    let device_names: Vec<&str> = devices.iter().map(|d| d.device.as_str()).collect();
    let unique_devices: HashSet<&str> = device_names.iter().cloned().collect();
    if device_names.len() != unique_devices.len() {
        return Err(ShapleyError::DataInconsistency(
            "There are duplicated devices in the list.".to_string(),
        ));
    }

    // Check that every device in private_links appears in devices
    let device_set: HashSet<&str> = devices.iter().map(|d| d.device.as_str()).collect();
    for link in private_links {
        if !device_set.contains(link.device1.as_str()) {
            return Err(ShapleyError::MissingDevice(link.device1.clone()));
        }
        if !device_set.contains(link.device2.as_str()) {
            return Err(ShapleyError::MissingDevice(link.device2.clone()));
        }
    }

    // Check that all demand nodes are reachable by the public network
    let public_nodes: HashSet<&str> = public_links
        .iter()
        .flat_map(|link| [link.city1.as_str(), link.city2.as_str()])
        .collect();

    for demand in demands {
        if !public_nodes.contains(demand.start.as_str()) {
            return Err(ShapleyError::UnreachableDemandNode(demand.start.clone()));
        }
        if !public_nodes.contains(demand.end.as_str()) {
            return Err(ShapleyError::UnreachableDemandNode(demand.end.clone()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Demand, Device, PrivateLink, PublicLink};

    #[test]
    fn test_valid_inputs() {
        let private_links = vec![PrivateLink::new(
            "SIN1".to_string(),
            "FRA1".to_string(),
            50.0,
            10.0,
            1.0,
            None,
        )];

        let devices = vec![
            Device::new("SIN1".to_string(), 1, "Alpha".to_string()),
            Device::new("FRA1".to_string(), 1, "Beta".to_string()),
        ];

        let public_links = vec![PublicLink::new("SIN".to_string(), "FRA".to_string(), 100.0)];

        let demands = vec![Demand::new(
            "SIN".to_string(),
            "FRA".to_string(),
            1,
            1.0,
            1.0,
            1,
            false,
        )];

        assert!(check_inputs(&private_links, &devices, &demands, &public_links, 1.0).is_ok());
    }

    #[test]
    fn test_too_many_operators() {
        let private_links = vec![PrivateLink::new(
            "A1".to_string(),
            "B1".to_string(),
            50.0,
            10.0,
            1.0,
            None,
        )];

        let mut devices = vec![];
        for i in 0..25 {
            devices.push(Device::new(format!("D{i}"), 1, format!("Op{i}")));
        }

        let public_links = vec![PublicLink::new("A".to_string(), "B".to_string(), 100.0)];

        let demands = vec![Demand::new(
            "A".to_string(),
            "B".to_string(),
            1,
            1.0,
            1.0,
            1,
            false,
        )];

        let result = check_inputs(&private_links, &devices, &demands, &public_links, 1.0);
        assert!(matches!(result, Err(ShapleyError::TooManyOperators { .. })));
    }
}
