#[allow(unused_imports)]
use sp1_build::{build_program_with_args, BuildArgs};
use sp1_sdk::ProverClient;

fn main() {
    build_program_with_args(
        "../program",
        BuildArgs {
            docker: true,
            //elf_name: Some("../elf/nori-sp1-helios-elf".to_string()),
            tag: "v4.1.3".to_string(),
    //        docker: true,
            output_directory: Some("../nori-elf".to_string()),
            ..Default::default()
        },
    );

     const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf"); // nori-elf/sp1-helios-program
     let client = ProverClient::from_env();
     let (pk, vk) = client.setup(ELF);

}
