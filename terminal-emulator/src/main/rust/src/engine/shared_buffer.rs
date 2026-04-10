/// 共享屏幕缓冲区

#[derive(Clone, Copy)]
pub struct SharedBufferPtr(pub *mut SharedScreenBuffer);
unsafe impl Send for SharedBufferPtr {}
unsafe impl Sync for SharedBufferPtr {}

#[repr(C)]
pub struct SharedScreenBuffer {
    pub version: u32,
    pub cols: u32,
    pub rows: u32,
    pub style_offset: u32,
    pub text_data: [u16; 0],
}

impl SharedScreenBuffer {
    pub fn required_size(cols: usize, rows: usize) -> usize {
        let header_size = 16;
        let text_size = cols * rows * 2;
        let aligned_text_size = (text_size + 7) & !7;
        let style_size = cols * rows * 8;
        header_size + aligned_text_size + style_size
    }

    pub fn style_data_ptr(&self) -> *const u64 {
        let cell_count = self.cols as usize * self.rows as usize;
        let text_size = cell_count * 2;
        let aligned_text_size = (text_size + 7) & !7;
        unsafe { (self.text_data.as_ptr() as *const u8).add(aligned_text_size) as *const u64 }
    }
}

pub struct FlatScreenBuffer {
    pub text_data: Vec<u16>,
    pub style_data: Vec<u64>,
    pub cols: usize,
    pub rows: usize,
}

impl FlatScreenBuffer {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cell_count = cols * rows;
        Self {
            text_data: vec![0u16; cell_count],
            style_data: vec![0u64; cell_count],
            cols,
            rows,
        }
    }

    pub fn create_shared_buffer(&self) -> *mut SharedScreenBuffer {
        let size = SharedScreenBuffer::required_size(self.cols, self.rows);
        unsafe {
            let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
            let ptr = std::alloc::alloc(layout) as *mut SharedScreenBuffer;
            if !ptr.is_null() {
                (*ptr).version = 0;
                (*ptr).cols = self.cols as u32;
                (*ptr).rows = self.rows as u32;
            }
            ptr
        }
    }

    pub fn cell_index(&self, col: usize, row: usize) -> usize {
        row * self.cols + col
    }

    pub fn get_cell(&self, col: usize, row: usize) -> (u16, u64) {
        let idx = self.cell_index(col, row);
        (self.text_data[idx], self.style_data[idx])
    }

    pub unsafe fn sync_to_shared(&self, shared_ptr: *mut SharedScreenBuffer) {
        if shared_ptr.is_null() { return; }
        unsafe {
            let base_ptr = shared_ptr as *mut u8;
            std::ptr::write(base_ptr.add(4) as *mut u32, self.cols as u32);
            std::ptr::write(base_ptr.add(8) as *mut u32, self.rows as u32);
            let text_size = (self.cols * self.rows * 2) as usize;
            let aligned_text_size = (text_size + 7) & !7;
            let style_offset = (16 + aligned_text_size) as u32;
            std::ptr::write(base_ptr.add(12) as *mut u32, style_offset);

            let cell_count = self.cols * self.rows;
            if cell_count > 0 {
                std::ptr::copy_nonoverlapping(self.text_data.as_ptr(), base_ptr.add(16) as *mut u16, cell_count);
                std::ptr::copy_nonoverlapping(self.style_data.as_ptr(), base_ptr.add(style_offset as usize) as *mut u64, cell_count);
            }
            let version_ptr = base_ptr.add(0) as *mut u32;
            let old_version = std::ptr::read(version_ptr);
            std::ptr::write(version_ptr, old_version.wrapping_add(1));
        }
    }
}
