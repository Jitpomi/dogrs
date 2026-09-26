//! Newline-delimited JSON commands. One DogRequest per line, one response per line.
//! Credentials follow the same authentication hooks as other external transports.
use crate::{CliOptions, IntoDogService};
use dog_core::{DogApp, DogError, DogRequest, DogTransportKind};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

const MAX_LINE: usize = 10 * 1024 * 1024;

pub struct DogCliService<R: Send + 'static, P: Send + Clone + 'static> {
    app: DogApp<R, P>,
    options: CliOptions,
}

impl<R, P> IntoDogService<CliOptions> for DogApp<R, P>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    type Service = DogCliService<R, P>;
    fn into_service(self, options: CliOptions) -> Self::Service {
        DogCliService { app: self, options }
    }
}

impl<R, P> DogCliService<R, P>
where
    R: Serialize + DeserializeOwned + Send + Sync + 'static,
    P: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
{
    pub async fn run_stdio(&self) -> std::io::Result<()> {
        if self.options.interactive.unwrap_or(false) {
            eprintln!("Enter one DogRequest JSON object per line. End input to exit.");
        }
        self.run(
            tokio::io::BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        )
        .await
    }

    /// Invalid commands return an error object and leave the session usable.
    /// An oversized line closes the session without allocating an unbounded buffer.
    pub async fn run(
        &self,
        mut input: impl AsyncBufRead + Unpin,
        mut output: impl AsyncWrite + Unpin,
    ) -> std::io::Result<()> {
        loop {
            let mut line = Vec::new();
            loop {
                let buf = input.fill_buf().await?;
                if buf.is_empty() {
                    break;
                }
                let count = buf
                    .iter()
                    .position(|b| *b == b'\n')
                    .map(|n| n + 1)
                    .unwrap_or(buf.len());
                if line.len() + count > MAX_LINE {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Command exceeds 10 MiB",
                    ));
                }
                let done = buf[count - 1] == b'\n';
                line.extend_from_slice(&buf[..count]);
                input.consume(count);
                if done {
                    break;
                }
            }
            if line.is_empty() {
                return Ok(());
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let result = match serde_json::from_slice::<DogRequest>(&line) {
                Ok(mut req) => {
                    req.transport = DogTransportKind::Cli;
                    self.app.handle(req).await
                }
                Err(_) => Err(DogError::bad_request("Invalid DogRequest JSON")),
            };
            let value = match result {
                Ok(response) => serde_json::to_value(response).map_err(std::io::Error::other)?,
                Err(error) => serde_json::json!({ "error": error.sanitize_for_client().to_json() }),
            };
            let mut bytes = serde_json::to_vec(&value).map_err(std::io::Error::other)?;
            bytes.push(b'\n');
            output.write_all(&bytes).await?;
            output.flush().await?;
        }
    }
}
