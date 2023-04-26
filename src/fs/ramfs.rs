// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2023 SUSE LLC
//
// Author: Joerg Roedel <jroedel@suse.de>

extern crate alloc;
use super::*;
use crate::error::SvsmError;
use crate::locking::RWLock;
use crate::mm::{allocate_file_page_ref, PageRef};
use crate::types::PAGE_SIZE;
use crate::utils::{page_align_up, page_offset, zero_mem_region};

use alloc::vec::Vec;
use core::cmp::{max, min};

struct RawRamFile {
    capacity: usize,
    size: usize,
    pages: Vec<PageRef>,
}

impl RawRamFile {
    pub fn new() -> Self {
        RawRamFile {
            capacity: 0,
            size: 0,
            pages: Vec::new(),
        }
    }

    fn increase_capacity(&mut self) -> Result<(), SvsmError> {
        let page_ref = allocate_file_page_ref()?;
        self.pages.push(page_ref);
        self.capacity += PAGE_SIZE;
        Ok(())
    }

    fn set_capacity(&mut self, capacity: usize) -> Result<(), SvsmError> {
        let cap = page_align_up(capacity);

        while cap > self.capacity {
            self.increase_capacity()?;
        }

        Ok(())
    }

    fn read_from_page(&self, buf: &mut [u8], offset: usize) {
        let page_index = page_offset(offset);
        let index = offset / PAGE_SIZE;
        let len = buf.len();
        let page_end = page_index + len;

        assert!(page_end <= PAGE_SIZE);

        let page_buf = self.pages[index].as_ref();
        buf.copy_from_slice(&page_buf[page_index..page_end]);
    }

    fn write_to_page(&mut self, buf: &[u8], offset: usize) {
        let page_index = page_offset(offset);
        let index = offset / PAGE_SIZE;
        let len = buf.len();
        let page_end = page_index + len;

        assert!(page_end <= PAGE_SIZE);

        let page_buf = self.pages[index].as_mut_ref();
        page_buf[page_index..page_end].copy_from_slice(buf);
    }

    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize, SvsmError> {
        let mut current = min(offset, self.size);
        let mut len = buf.len();
        let mut bytes: usize = 0;
        let mut buf_offset = 0;

        while len > 0 {
            let page_end = min(page_align_up(current + 1), self.size);
            let page_len = min(page_end - current, len);
            let buf_end = buf_offset + page_len;

            if page_len == 0 {
                break;
            }

            self.read_from_page(&mut buf[buf_offset..buf_end], current);

            buf_offset = buf_end;
            current += page_len;
            len -= page_len;
            bytes += page_len;
        }

        Ok(bytes)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> Result<usize, SvsmError> {
        let mut current = offset;
        let mut bytes: usize = 0;
        let mut len = buf.len();
        let mut buf_offset: usize = 0;
        let capacity = offset
            .checked_add(len)
            .ok_or(SvsmError::FileSystem(FsError::inval()))?;

        self.set_capacity(capacity)?;

        while len > 0 {
            let page_len = min(PAGE_SIZE - page_offset(current), len);
            let buf_end = buf_offset + page_len;

            self.write_to_page(&buf[buf_offset..buf_end], current);
            self.size = max(self.size, current + page_len);

            current += page_len;
            buf_offset += page_len;
            len -= page_len;
            bytes += page_len;
        }

        Ok(bytes)
    }

    fn truncate(&mut self, size: usize) -> Result<usize, SvsmError> {
        if size > self.size {
            return Err(SvsmError::FileSystem(FsError::inval()));
        }

        let offset = page_offset(size);
        let base_pages = size / PAGE_SIZE;
        let new_pages = if offset > 0 {
            base_pages + 1
        } else {
            base_pages
        };

        // Clear pages and remove them from the file
        while self.pages.len() > new_pages {
            let page_ref = self.pages.pop().unwrap();
            let vaddr = page_ref.virt_addr();
            zero_mem_region(vaddr, vaddr + PAGE_SIZE);
        }

        self.capacity = new_pages * PAGE_SIZE;
        self.size = size;

        if offset > 0 {
            // Clear the last page after new EOF
            let page_ref = self.pages.last().unwrap();
            let vaddr = page_ref.virt_addr();
            zero_mem_region(vaddr + offset, vaddr + PAGE_SIZE);
        }

        Ok(size)
    }

    fn size(&self) -> usize {
        self.size
    }
}

pub struct RamFile {
    rawfile: RWLock<RawRamFile>,
}

unsafe impl Sync for RamFile {}

impl RamFile {
    #[allow(dead_code)]
    pub fn new() -> Self {
        RamFile {
            rawfile: RWLock::new(RawRamFile::new()),
        }
    }
}

impl File for RamFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> Result<usize, SvsmError> {
        self.rawfile.lock_read().read(buf, offset)
    }

    fn write(&self, buf: &[u8], offset: usize) -> Result<usize, SvsmError> {
        self.rawfile.lock_write().write(buf, offset)
    }

    fn truncate(&self, size: usize) -> Result<usize, SvsmError> {
        self.rawfile.lock_write().truncate(size)
    }

    fn size(&self) -> usize {
        self.rawfile.lock_read().size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mm::alloc::{destroy_test_root_mem, setup_test_root_mem, DEFAULT_TEST_MEMORY_SIZE};

    #[test]
    fn test_ramfs_file_read_write() {
        let test_mem_lock = setup_test_root_mem(DEFAULT_TEST_MEMORY_SIZE);

        let file = RamFile::new();
        let mut buf1 = [0xffu8; 512];

        // Write first buffer at offset 0
        file.write(&mut buf1, 0).expect("Failed to write file data");
        assert!(file.size() == 512);

        // Write second buffer at offset 4096 - 256 - cross-page write
        let mut buf2 = [0xaau8; 512];
        file.write(&mut buf2, PAGE_SIZE - 256)
            .expect("Failed to write file cross-page");
        assert!(file.size() == PAGE_SIZE + 256);

        // Clear buffer before reading into it
        buf1 = [0u8; 512];

        // Read back and check first buffer
        let size = file
            .read(&mut buf1, 0)
            .expect("Failed to read from offset 0");
        assert!(size == 512);

        for byte in buf1.iter() {
            assert!(*byte == 0xff);
        }

        // Clear buffer before reading into it
        buf2 = [0u8; 512];

        // Read back and check second buffer
        let size = file
            .read(&mut buf2, PAGE_SIZE - 256)
            .expect("Failed to read from offset PAGE_SIZE - 256");
        assert!(size == 512);

        for byte in buf2.iter() {
            assert!(*byte == 0xaa);
        }

        // Check complete file
        let mut buf3: [u8; 8192] = [0xcc; 8192];
        let size = file.read(&mut buf3, 0).expect("Failed to read whole file");
        assert!(size == PAGE_SIZE + 256);

        for i in 0..buf3.len() {
            let expected: u8 = if i < 512 {
                0xff
            } else if i < PAGE_SIZE - 256 {
                0
            } else if i < PAGE_SIZE + 256 {
                0xaa
            } else {
                0xcc
            };
            assert!(buf3[i] == expected);
        }

        assert_eq!(file.truncate(1024).unwrap(), 1024);
        assert_eq!(file.size(), 1024);

        // Clear buffer before reading again into it
        buf3 = [0u8; 8192];

        // read file again
        let size = file.read(&mut buf3, 0).expect("Failed to read whole file");
        assert!(size == 1024);

        for i in 0..1024 {
            let expected: u8 = if i < 512 { 0xff } else { 0 };
            assert!(buf3[i] == expected);
        }

        // file needs to be dropped before memory allocator is destroyed
        drop(file);

        destroy_test_root_mem(test_mem_lock);
    }
}
