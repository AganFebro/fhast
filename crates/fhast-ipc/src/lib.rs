mod error;
mod message;
mod transport;

pub use error::IpcError;
pub use message::{DownloadDto, EventDto, IpcRequest, IpcResponse, SegmentDto, IPC_VERSION};
pub use transport::{read_message, write_message};
