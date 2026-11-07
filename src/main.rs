mod adapter;
mod build;
mod cache;
mod cli;
mod config;
mod detect;
mod doctor;
mod exec;
mod manifest;
mod package;
mod plan;
mod target;
mod toolchain;
mod verify;

fn main() {
    std::process::exit(cli::run());
}
