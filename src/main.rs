mod handler;
mod io;
mod listener;
mod settings;

mod http;
mod tls;
mod tunnel;
mod error;

use futures::future::{join_all, try_join_all};
use settings::{build_listener, Settings};
use error::Error;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let settings = Settings::new()?;

    let listeners = try_join_all(settings.listeners.iter().map(build_listener)).await?;
    join_all(listeners.iter().map(|l| l.handle())).await;

    Ok(())
}
