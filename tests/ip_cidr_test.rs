use cdn_ip_tester::data::{Loadable, Subnet};

#[test]
fn parse_ip_cidr() {
    let subnets: Vec<Subnet> = Vec::from_str(
        r"192.168.1.1
        192.167.2.0/24
        192.167.3.3/24
        1.2.3.456/24
        1.2.3.4/24
        1.2.3.4a/24
        1.2.3.a5/12
        ",
    )
    .unwrap();
    println!("{subnets:?}");
    // assert_eq!(4, subnets);
}
