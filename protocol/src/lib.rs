use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InputEvent {
    MouseMove { dx: f64, dy: f64 },
    MouseDown { button: u32 },
    MouseUp { button: u32 },
    Scroll { dx: f64, dy: f64 },
    KeyDown { keycode: u32 },
    KeyUp { keycode: u32 },
    Clipboard { text: String },
}

pub async fn send_event<W>(writer: &mut W, event: &InputEvent) -> Res<()>
where
    W: AsyncWrite + Unpin,
{
    let encoded = bincode::serialize(event)?;
    let len = encoded.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn write_event<W>(writer: &mut W, event: &InputEvent) -> Res<()>
where
    W: AsyncWrite + Unpin,
{
    send_event(writer, event).await
}

pub async fn read_event<R>(reader: &mut R) -> Res<Option<InputEvent>>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    if reader.read_exact(&mut len_buf).await.is_err() {
        return Ok(None);
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let event = bincode::deserialize(&buf)?;
    Ok(Some(event))
}
