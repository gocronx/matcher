use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use crate::types::{Order, OrderId, ProductId};

/// 日志条目，代表所有可能改变状态的操作
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum LogEntry {
    PlaceOrder(Order),
    CancelOrder(OrderId, ProductId),
}

/// WAL 管理器，负责日志的写入和重放
pub struct WalManager {
    path: PathBuf,
    // 使用 Mutex 保护写入，确保线程安全
    // 虽然我们在 async 上下文中，但文件 I/O 这里使用 std::fs (同步)
    // 对于 WAL 这种顺序追加场景，同步写入配合 BufWriter 通常性能是可以接受的
    // 且比异步文件 I/O 更容易保证数据落盘顺序
    writer: Mutex<BufWriter<File>>,
}

impl WalManager {
    /// 打开或创建 WAL 文件
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&path)?;

        info!("WAL initialized at: {:?}", path);

        Ok(Self {
            path,
            writer: Mutex::new(BufWriter::new(file)),
        })
    }

    /// 追加一条日志
    pub fn append(&self, entry: &LogEntry) -> io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        
        // 使用 bincode 序列化写入
        // bincode 会自动处理长度或结构信息
        bincode::serialize_into(&mut *writer, entry)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        // 为了数据安全性，每次写入后应该 flush
        // 注意：这不会执行 fsync (sync_all)，只是将数据从 Rust 缓冲区冲刷到 OS 缓冲区
        // 如果需要极高的持久性保证，应该调用 writer.get_ref().sync_all()，但这会极大降低性能
        writer.flush()?;
        
        Ok(())
    }

    /// 重放日志，返回所有日志条目的迭代器
    pub fn replay(&self) -> io::Result<Vec<LogEntry>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        info!("Replaying WAL from: {:?}", self.path);

        loop {
            // 尝试读取下一个条目
            match bincode::deserialize_from(&mut reader) {
                Ok(entry) => {
                    entries.push(entry);
                }
                Err(e) => {
                    // bincode 错误处理比较麻烦，EOF 也是一个错误
                    // 我们需要判断这是否真的是 EOF
                    if let bincode::ErrorKind::Io(ref io_err) = *e {
                        if io_err.kind() == io::ErrorKind::UnexpectedEof {
                            // 正常结束
                            break;
                        }
                    }
                    // 如果读到一半出错，可能是上次崩溃导致的截断
                    // 在生产环境中，我们可能需要截断坏尾
                    warn!("WAL replay ended with potential corruption or EOF: {}", e);
                    break;
                }
            }
        }

        info!("WAL replay complete. Loaded {} entries.", entries.len());
        Ok(entries)
    }
}
