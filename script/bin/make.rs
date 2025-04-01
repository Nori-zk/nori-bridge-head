#[allow(unused_imports)]
use sp1_build::{build_program_with_args, BuildArgs};
use sp1_sdk::{HashableKey, ProverClient};

fn main() {
    build_program_with_args(
        "../program",
        BuildArgs {
            docker: true,
            tag: "v4.1.3".to_string(),
            output_directory: Some("../nori-elf".to_string()),
            ..Default::default()
        },
    );

    println!("ZK built.");

    const ELF: &[u8] = include_bytes!("../../nori-elf/sp1-helios-program");
    let client = ProverClient::from_env();
    let (pk, vk) = client.setup(ELF);

    //info!("PK: {}", pk.pk.constraints_map);
    println!("VK: {}", vk.vk.bytes32());
}
