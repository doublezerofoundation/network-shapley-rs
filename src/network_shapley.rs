use crate::{
    coalition_computation::{
        calculate_shapley_values, compute_expected_values, enumerate_operators,
        generate_coalition_bitmap, solve_coalition_values,
    },
    lp::{consolidate_map, primitives},
    types::{DemandMatrix, PrivateLinks, PublicLinks, Result, ShapleyValue},
    utils::decimal_to_f64,
    validation::validate_operator_names,
};
use derive_builder::Builder;
use faer::Par;
use rust_decimal::{Decimal, dec};

#[derive(Builder)]
pub struct NetworkShapley {
    private_links: PrivateLinks,
    public_links: PublicLinks,
    demand: DemandMatrix,
    #[builder(default = "dec!(0.98)")]
    operator_uptime: Decimal,
    #[builder(default = "dec!(5.0)")]
    hybrid_penalty: Decimal,
    #[builder(default = "dec!(1.0)")]
    demand_multiplier: Decimal,
}

impl NetworkShapley {
    /// Compute Shapley values per operator
    pub fn compute(&self) -> Result<Vec<ShapleyValue>> {
        // Configure faer to use all available threads for matrix operations
        faer::set_global_parallelism(Par::rayon(0));

        // Enumerate operators and validate
        let operators = enumerate_operators(&self.private_links.links);
        validate_operator_names(&operators)?;
        let n_ops = operators.len();

        // Generate coalition bitmap
        let bitmap = generate_coalition_bitmap(n_ops);

        // Get LP primitives
        let full_map = consolidate_map(
            &self.private_links,
            &self.public_links,
            &self.demand,
            self.hybrid_penalty,
        )?;
        let primitives = primitives(&full_map, &self.demand, self.demand_multiplier)?;

        // Solve for coalition values
        let (svalue, size) = solve_coalition_values(&operators, &bitmap, &primitives)?;

        // Compute expected values with downtime
        let evalue =
            compute_expected_values(&svalue, &size, decimal_to_f64(self.operator_uptime), n_ops)?;

        // Calculate Shapley values
        calculate_shapley_values(&operators, &evalue, &size, n_ops)
    }
}

#[cfg(test)]
mod tests {
    use crate::{DemandBuilder, LinkBuilder, lp};

    use super::*;
    use rust_decimal::dec;

    fn create_example_private_links() -> PrivateLinks {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("FRA1".to_string())
                    .end("NYC1".to_string())
                    .cost(dec!(40))
                    .bandwidth(dec!(10))
                    .operator1("Alpha".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("FRA1".to_string())
                    .end("SIN1".to_string())
                    .cost(dec!(50))
                    .bandwidth(dec!(10))
                    .operator1("Beta".to_string())
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("SIN1".to_string())
                    .end("NYC1".to_string())
                    .cost(dec!(80))
                    .bandwidth(dec!(10))
                    .operator1("Gamma".to_string())
                    .build()
                    .unwrap()
            },
        ];
        PrivateLinks::from_links(links)
    }

    fn create_example_public_links() -> PublicLinks {
        let links = vec![
            {
                LinkBuilder::default()
                    .start("FRA1".to_string())
                    .end("NYC1".to_string())
                    .cost(dec!(70))
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("FRA1".to_string())
                    .end("SIN1".to_string())
                    .cost(dec!(80))
                    .build()
                    .unwrap()
            },
            {
                LinkBuilder::default()
                    .start("SIN1".to_string())
                    .end("NYC1".to_string())
                    .cost(dec!(120))
                    .build()
                    .unwrap()
            },
        ];
        PublicLinks::from_links(links)
    }

    fn create_example_demand() -> DemandMatrix {
        let demands = vec![
            DemandBuilder::default()
                .start("SIN".to_string())
                .end("NYC".to_string())
                .traffic(dec!(5))
                .demand_type(1)
                .build()
                .unwrap(),
            DemandBuilder::default()
                .start("SIN".to_string())
                .end("FRA".to_string())
                .traffic(dec!(5))
                .demand_type(1)
                .build()
                .unwrap(),
        ];
        DemandMatrix::from_demands(demands)
    }

    #[test]
    fn test_consolidate_map() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let result = consolidate_map(&private_links, &public_links, &demand, dec!(5)).unwrap();

        // Should have private links (bidirectional), public links (bidirectional), and helper links
        assert!(result.len() > 6); // At least 6 for bidirectional private links

        // Check that private links are first and bidirectional
        assert_eq!(result[0].operator1, "Alpha");

        // Find the reverse of the first link
        let reverse_found = result
            .iter()
            .any(|link| link.start == "NYC1" && link.end == "FRA1" && link.operator1 == "Alpha");
        assert!(reverse_found);
    }

    #[test]
    fn test_lp_primitives() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let link_map = consolidate_map(&private_links, &public_links, &demand, dec!(5)).unwrap();
        let primitives = lp::primitives(&link_map, &demand, dec!(1)).unwrap();

        // Check that matrices have appropriate dimensions
        assert!(primitives.a_eq.nrows() > 0);
        assert!(primitives.a_eq.ncols() > 0);
        assert_eq!(primitives.b_eq.nrows(), primitives.a_eq.nrows());

        // Check operator indices
        assert!(!primitives.col_index1.is_empty());
        assert_eq!(primitives.col_index1.len(), primitives.col_index2.len());
    }

    #[test]
    fn test_network_shapley_example() {
        let private_links = create_example_private_links();
        let public_links = create_example_public_links();
        let demand = create_example_demand();

        let result = NetworkShapleyBuilder::default()
            .private_links(private_links)
            .public_links(public_links)
            .demand(demand)
            .build()
            .unwrap()
            .compute()
            .unwrap();

        // Should have 3 operators
        assert_eq!(result.len(), 3);

        // Check operator names
        let operators: Vec<&str> = result.iter().map(|sv| sv.operator.as_str()).collect();
        assert!(operators.contains(&"Alpha"));
        assert!(operators.contains(&"Beta"));
        assert!(operators.contains(&"Gamma"));

        // Percentages should sum to 1
        let total: Decimal = result.iter().map(|sv| sv.percent).sum();
        assert_eq!(total, dec!(1.0));

        // All percentages should be non-negative
        assert!(result.iter().all(|sv| sv.percent >= dec!(0)));
    }
}
