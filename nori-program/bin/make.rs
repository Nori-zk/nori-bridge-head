#[allow(unused_imports)]
use sp1_build::{build_program_with_args, BuildArgs};
use sp1_sdk::ProverClient;

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
}
