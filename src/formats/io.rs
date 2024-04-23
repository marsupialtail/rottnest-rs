use bytes::{Bytes, BytesMut};
#[cfg(feature = "opendal")]
use opendal::{services::{Fs, S3}, Reader};
use std::env;
use std::io::{Read, SeekFrom};
use std::ops::{Deref, DerefMut};
use tokio::{io::AsyncRead, pin};
use zstd::stream::read::Decoder;

use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::lava::error::LavaError;

pub const READER_BUFFER_SIZE: usize = 4 * 1024 * 1024;
pub const WRITER_BUFFER_SIZE: usize = 4 * 1024 * 1024;

pub struct AsyncReader {
    reader: Reader,
    pub filename: String,
}

// impl Deref for AsyncReader {
//     type Target = Reader;

//     fn deref(&self) -> &Self::Target {
//         &self.reader
//     }
// }

// impl DerefMut for AsyncReader {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.reader
//     }
// }

// impl From<Reader> for AsyncReader {
//     fn from(reader: Reader) -> Self {
//         Self::new(reader)
//     }
// }

impl AsyncReader {
    pub fn new(reader: Reader, filename: String) -> Self {
        Self { reader, filename }
    }

    pub async fn read_range(&mut self, from: u64, to: u64) -> Result<Bytes, LavaError> {
        if from >= to {
            return Err(LavaError::Io(std::io::ErrorKind::InvalidData.into()));
        }

        let reader = self;
        pin!(reader);

        let mut current = 0;
        let total = to - from;
        let mut res = BytesMut::with_capacity(total as usize);

        while current < total {
            let mut buffer = res.split_off(current as usize);
            reader.seek(SeekFrom::Start(from + current)).await?;
            let size = reader.read_buf(&mut buffer).await?;
            // reader.read_exact(buf)
            res.unsplit(buffer);
            current += size as u64;
        }

        if res.len() < total as usize {
            return Err(LavaError::Io(std::io::ErrorKind::Interrupted.into()));
        }

        Ok(res.freeze())
    }

    // theoretically we should try to return different types here, but Vec<u64> is def. the most common
    pub async fn read_range_and_decompress(
        &mut self,
        from: u64,
        to: u64,
    ) -> Result<Vec<u64>, LavaError> {
        let compressed_posting_list_offsets = self.read_range(from, to).await?;
        let mut decompressor = Decoder::new(&compressed_posting_list_offsets[..])?;
        let mut serialized_posting_list_offsets: Vec<u8> =
            Vec::with_capacity(compressed_posting_list_offsets.len() as usize);
        decompressor.read_to_end(&mut serialized_posting_list_offsets)?;
        let result: Vec<u64> = bincode::deserialize(&serialized_posting_list_offsets)?;
        Ok(result)
    }

    pub async fn read_usize_from_end(&mut self, n: u64) -> Result<Vec<u64>, LavaError> {
        let reader = self;
        pin!(reader);
        reader.seek(SeekFrom::End(-(n as i64 * 8))).await?;
        let mut result: Vec<u64> = vec![];
        for i in 0..n {
            result.push(reader.read_u64_le().await?);
        }
        Ok(result)
    }
}


pub(crate) enum Config {
    #[cfg(feature = "opendal")]
    OpendalFs(opendal::services::Fs),
    #[cfg(feature = "opendal")]
    OpendalS3(opendal::services::S3),
    #[cfg(feature = "aws_sdk")]
    Aws(aws_config::SdkConfig),
}

#[cfg(feature = "opendal")]
impl From<&str> for Config {
    fn from(file: &str) -> Self {
        if file.starts_with("s3://") {
            let mut builder = S3::default();
            let mut iter = file[5..].split("/");

            builder.bucket(iter.next().expect("malformed path"));
            // Set the region. This is required for some services, if you don't care about it, for example Minio service, just set it to "auto", it will be ignored.
            if let Ok(value) = env::var("AWS_ENDPOINT_URL") {
                builder.endpoint(&value);
            }
            if let Ok(value) = env::var("AWS_REGION") {
                builder.region(&value);
            }
            if let Ok(_value) = env::var("AWS_VIRTUAL_HOST_STYLE") {
                builder.enable_virtual_host_style();
            }
            return Config::OpendalS3(builder);
        } else {
            let mut builder = Fs::default();
            // let current_path = env::current_dir().expect("no path");
            builder.root(folder);
            return Config::OpendalFs(builder);
        }
    }
}

impl Config {
    #[cfg(feature = "aws_sdk")]
    pub async fn from_env() -> Self {
        let config = aws_config::load_from_env().await;
        Config::Aws(config)
    }
}

#[derive(Clone)]
pub(crate) enum Operator {
    #[cfg(feature = "opendal")]
    Opendal(opendal::Operator),
    #[cfg(feature = "aws_sdk")]
    Aws(aws_sdk_s3::Client),
}

impl From<Config> for Operator {
    fn from(config: Config) -> Self {
        match config {
            #[cfg(feature = "opendal")]
            Config::OpendalFs(fs) => Operator::Opendal(opendal::Operator::new(fs).expect("Fs Builder construction error").finish()),
            #[cfg(feature = "opendal")]
            Config::OpendalS3(s3) => Operator::Opendal(opendal::Operator::new(s3).expect("S3 Builder construction error").finish()),
            #[cfg(feature = "aws_sdk")]
            Config::Aws(config) => Operator::Aws(aws_sdk_s3::Client::new(&config)),
        }
    }
}

impl Operator {

}

#[cfg(feature = "aws_sdk")]
pub(crate) async fn get_file_sizes_and_readers(
    files: &[String],
) -> Result<(Vec<usize>, Vec<AsyncReader>), LavaError> {
    let config = Config::from_env().await;
    let operator = Operator::from(config);
    let tasks: Vec<_> = files
        .iter()
        .map(|file| {
            let file = file.clone(); // Clone file name to move into the async block
            let operator = operator.clone();
            tokio::spawn(async move {
                // Extract filename
                let filename = if file.starts_with("s3://") {
                    file[5..].split('/').collect::<Vec<_>>()[1..].join("/")
                } else {
                    file.clone()
                };

                // Create the reader
                let reader: AsyncReader = AsyncReader::new(
                    operator
                        .clone()
                        .reader_with(&filename)
                        .buffer(READER_BUFFER_SIZE)
                        .await?,
                    filename.clone(),
                );

                // Get the file size
                let file_size: u64 = operator.stat(&filename).await?.content_length();

                Ok::<_, LavaError>((file_size as usize, reader))
            })
        })
        .collect();

    // Wait for all tasks to complete
    let results = futures::future::join_all(tasks).await;

    // Process results, separating out file sizes and readers
    let mut file_sizes = Vec::new();
    let mut readers = Vec::new();

    for result in results {
        match result {
            Ok(Ok((size, reader))) => {
                file_sizes.push(size);
                readers.push(reader);
            }
            Ok(Err(e)) => return Err(e), // Handle error from inner task
            Err(e) => return Err(LavaError::Parse("Task join error: {}".to_string())), // Handle join error
        }
    }

    Ok((file_sizes, readers))
}


#[cfg(feature = "opendal")]
pub(crate) async fn get_file_sizes_and_readers(
    files: &[String],
) -> Result<(Vec<usize>, Vec<AsyncReader>), LavaError> {
    let tasks: Vec<_> = files
        .iter()
        .map(|file| {
            let file = file.clone(); // Clone file name to move into the async block

            tokio::spawn(async move {
                // Determine the operator based on the file scheme
                let operator = if file.starts_with("s3://") {
                    Operators::from(S3Builder::from(file.as_str())).into_inner()
                } else {
                    let current_path = env::current_dir()?;
                    Operators::from(FsBuilder::from(current_path.to_str().expect("no path")))
                        .into_inner()
                };

                // Extract filename
                let filename = if file.starts_with("s3://") {
                    file[5..].split('/').collect::<Vec<_>>()[1..].join("/")
                } else {
                    file.clone()
                };

                // Create the reader
                let reader: AsyncReader = AsyncReader::new(
                    operator
                        .clone()
                        .reader_with(&filename)
                        .buffer(READER_BUFFER_SIZE)
                        .await?,
                    filename.clone(),
                );

                // Get the file size
                let file_size: u64 = operator.stat(&filename).await?.content_length();

                Ok::<_, LavaError>((file_size as usize, reader))
            })
        })
        .collect();

    // Wait for all tasks to complete
    let results = futures::future::join_all(tasks).await;

    // Process results, separating out file sizes and readers
    let mut file_sizes = Vec::new();
    let mut readers = Vec::new();

    for result in results {
        match result {
            Ok(Ok((size, reader))) => {
                file_sizes.push(size);
                readers.push(reader);
            }
            Ok(Err(e)) => return Err(e), // Handle error from inner task
            Err(e) => return Err(LavaError::Parse("Task join error: {}".to_string())), // Handle join error
        }
    }

    Ok((file_sizes, readers))
}
