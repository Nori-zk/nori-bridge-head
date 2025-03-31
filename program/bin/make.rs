#[allow(unused_imports)]
use sp1_build::{build_program_with_args, BuildArgs};
use sp1_sdk::ProverClient;

fn main() {
     build_program_with_args("../program", BuildArgs {
         tag: "v4.0.0-rc.3".to_string(),
         docker: true,
         elf_name: Some("sp1-helios-elf".to_string()),
         ..Default::default()
     });

     const ELF: &[u8] = include_bytes!("../../elf/sp1-helios-elf");
     let client = ProverClient::from_env();
     let (pk, vk) = client.setup(ELF);

}
