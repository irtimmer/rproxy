use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};

use std::error::Error;

mod settings;

use settings::Settings;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::new()?;

    println!("Listening on: {}", settings.listen);
    println!("Proxying to: {}", settings.target);

    let listener = TcpListener::bind(settings.listen).await?;

    while let Ok((mut inbound, _)) = listener.accept().await {
        let mut outbound = TcpStream::connect(settings.target.clone()).await?;

        tokio::spawn(async move {
            let r = copy_bidirectional(&mut inbound, &mut outbound).await;
            if let Err(e) = r {
                println!("Failed to transfer; error={}", e);
            }
        });
    }

    Ok(())
}
