use std::fs::{File, OpenOptions};
use std::io::{self, Write, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};
use log::{error, debug};
use crate::batch::{Batch, BatchDirection};

pub struct BatchHistory {
    file: Arc<Mutex<File>>,
    current_batch: u64,
}

impl BatchHistory {
    pub fn new(history_path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .append(true)
            .open(history_path)?;
        
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            current_batch: 0,
        })
    }

    pub fn save_batch(&mut self, batch: &Batch) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        
        // Write batch number (8 bytes)
        file.write_all(&batch.number.to_le_bytes())?;
        
        // Write direction (1 byte)
        file.write_all(&[match batch.direction {
            BatchDirection::Incoming => 0,
            BatchDirection::Outgoing => 1,
        }])?;
        
        // Write data length (8 bytes)
        file.write_all(&(batch.data.len() as u64).to_le_bytes())?;
        
        // Write the actual data
        file.write_all(&batch.data)?;
        
        // Flush to ensure data is written to disk
        file.flush()?;
        
        self.current_batch = batch.number;
        debug!("Saved batch {} to history file", batch.number);
        Ok(())
    }

    pub fn get_batches_since(&self, batch_number: u64) -> io::Result<Vec<Batch>> {
        let mut file = self.file.lock().unwrap();
        let mut batches = Vec::new();
        
        // Seek to start of file
        file.seek(SeekFrom::Start(0))?;
        
        loop {
            // Read batch number (8 bytes)
            let mut batch_num_buf = [0u8; 8];
            match file.read_exact(&mut batch_num_buf) {
                Ok(_) => {
                    let batch_num = u64::from_le_bytes(batch_num_buf);
                    
                    // Read direction (1 byte)
                    let mut direction_buf = [0u8; 1];
                    if file.read_exact(&mut direction_buf).is_err() {
                        error!("Failed to read batch direction, file may be corrupted");
                        break;
                    }
                    let direction = match direction_buf[0] {
                        0 => BatchDirection::Incoming,
                        1 => BatchDirection::Outgoing,
                        _ => {
                            error!("Invalid batch direction in history file");
                            break;
                        }
                    };
                    
                    // Read data length (8 bytes)
                    let mut len_buf = [0u8; 8];
                    if file.read_exact(&mut len_buf).is_err() {
                        error!("Failed to read batch data length, file may be corrupted");
                        break;
                    }
                    let data_len = u64::from_le_bytes(len_buf) as usize;
                    
                    // Read the data
                    let mut data = vec![0u8; data_len];
                    if file.read_exact(&mut data).is_err() {
                        error!("Failed to read batch data, file may be corrupted");
                        break;
                    }
                    
                    // Only add batches after the requested number
                    if batch_num > batch_number {
                        batches.push(Batch {
                            number: batch_num,
                            direction,
                            data,
                        });
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    // Normal EOF, we're done
                    break;
                }
                Err(e) => {
                    error!("Error reading batch history: {}", e);
                    return Err(e);
                }
            }
        }
        
        debug!("Retrieved {} batches since batch {}", batches.len(), batch_number);
        Ok(batches)
    }

    pub fn get_current_batch(&self) -> u64 {
        self.current_batch
    }
} 