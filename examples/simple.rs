use rust_decimal::dec;
use shapley::{
    DemandMatrix, LinkBuilder, NetworkShapleyBuilder, PrivateLinks, PublicLinks, error::Result,
    types::DemandBuilder,
};

fn build_sample_inputs() -> Result<(PrivateLinks, PublicLinks, DemandMatrix)> {
    // Private links
    let private_links = PrivateLinks::from_links(vec![
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(40))
                .bandwidth(dec!(10))
                .operator1("Alpha".to_string())
                .build()?
        },
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("SIN1".to_string())
                .cost(dec!(50))
                .bandwidth(dec!(10))
                .operator1("Beta".to_string())
                .build()?
        },
        {
            LinkBuilder::default()
                .start("SIN1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(80))
                .bandwidth(dec!(10))
                .operator1("Gamma".to_string())
                .build()?
        },
    ]);

    // Public links
    let public_links = PublicLinks::from_links(vec![
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(70))
                .build()?
        },
        {
            LinkBuilder::default()
                .start("FRA1".to_string())
                .end("SIN1".to_string())
                .cost(dec!(80))
                .build()?
        },
        {
            LinkBuilder::default()
                .start("SIN1".to_string())
                .end("NYC1".to_string())
                .cost(dec!(120))
                .build()?
        },
    ]);

    // Demand
    let demand = DemandMatrix::from_demands(vec![
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
    ]);

    Ok((private_links, public_links, demand))
}

fn main() -> Result<()> {
    let (private_links, public_links, demand) = build_sample_inputs()?;
    let result = NetworkShapleyBuilder::default()
        .private_links(private_links)
        .public_links(public_links)
        .demand(demand)
        .build()?
        .compute()?;

    println!("{:>9}  {:>9}  {:>9}", "Operator", "Value", "Percent");
    for sv in result {
        println!(
            "{:>9}  {:>9}  {:>9.2}%",
            sv.operator,
            sv.value,
            sv.percent * dec!(100)
        );
    }

    Ok(())
}
