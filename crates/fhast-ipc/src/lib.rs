mod connection;
mod error;
mod message;
mod transport;

pub use connection::{bind_daemon_socket, connect_to_daemon, split_stream, IpcListener, IpcStream};
pub use error::IpcError;
pub use message::{DownloadDto, EventDto, IpcRequest, IpcResponse, SegmentDto, IPC_VERSION};
pub use transport::{read_message, write_message};
