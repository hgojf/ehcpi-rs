use ehcpi_rs::*;
use daemonize::Daemonize;

fn main() {
	Daemonize::new()
	.start()
	.expect("failed to daemonize");
	run();
}
