use ehcpi_rs::*;
use daemonize::Daemonize;

fn main() {
	Daemonize::new()
	.start()
	.expect("failed to daemonize");
	tokio_main();
}

#[tokio::main]
async fn tokio_main() {
	run().await;
}
