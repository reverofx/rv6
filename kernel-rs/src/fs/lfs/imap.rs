use super::Segment;
use crate::{
    bio::Buf,
    hal::hal,
    param::{BSIZE, IMAPSIZE},
    proc::KernelCtx,
};

// Number of entries in each on-disk imap block.
pub const NENTRY: usize = BSIZE / 4;

/// On-disk structure for each imap block.
/// Stores the disk block number for each inum.
#[repr(C)]
#[derive(Clone)]
struct DImapBlock {
    entry: [u32; NENTRY],
}

/// Stores the address of each imap block.
pub struct Imap {
    dev_no: u32,
    addr: [u32; IMAPSIZE],
}

impl const Default for Imap {
    fn default() -> Self {
        Self {
            dev_no: 0,
            addr: [0; IMAPSIZE],
        }
    }
}

impl Imap {
    #[allow(dead_code)]
    pub fn new(dev_no: u32, addr: [u32; IMAPSIZE]) -> Self {
        Self { dev_no, addr }
    }

    /// For the inode with inode number `inum`,
    /// returns where the inode's mapping is stored in the imap in the form of (imap block number, offset within block).
    fn get_imap_block_no(&self, inum: u32) -> (usize, usize) {
        (inum as usize / NENTRY, inum as usize % NENTRY)
    }

    /// Returns the `block_no`th block of the imap.
    fn get_imap_block(&self, block_no: usize, ctx: &KernelCtx<'_, '_>) -> Buf {
        hal().disk().read(self.dev_no, self.addr[block_no], ctx)
    }

    /// Returns an unused inum.
    pub fn get_empty_inum(&self, ctx: &KernelCtx<'_, '_>) -> Option<u32> {
        for i in 0..IMAPSIZE {
            let buf = self.get_imap_block(i, ctx);
            let imap_block = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DImapBlock) };
            for j in 0..NENTRY {
                if imap_block.entry[j] == 0 {
                    return Some((i * NENTRY + j) as u32);
                }
            }
            buf.free(ctx);
        }
        None
    }

    /// For the inode with inode number `inum`, returns the disk_block_no of it.
    #[allow(dead_code)]
    pub fn get(&self, inum: u32, ctx: &KernelCtx<'_, '_>) -> u32 {
        assert!(
            inum < ctx.kernel().fs().superblock().ninodes,
            "invalid inum"
        );
        let (block_no, offset) = self.get_imap_block_no(inum);
        let buf = self.get_imap_block(block_no, ctx);

        let imap_block = unsafe { &*(buf.deref_inner().data.as_ptr() as *const DImapBlock) };
        let res = imap_block.entry[offset];
        buf.free(ctx);
        res
    }

    /// For the inode with inode number `inum`, updates its mapping in the imap to disk_block_no.
    /// Then, we append the new imap block to the segment.
    /// Returns true if successful. Otherwise, returns false.
    pub fn set(
        &mut self,
        inum: u32,
        disk_block_no: u32,
        segment: &mut Segment,
        ctx: &KernelCtx<'_, '_>,
    ) -> bool {
        assert!(
            inum < ctx.kernel().fs().superblock().ninodes,
            "invalid inum"
        );
        let (block_no, offset) = self.get_imap_block_no(inum);

        if let Some((mut buf, new_addr)) = segment.get_or_add_imap_block(block_no as u32, ctx) {
            let imap_block =
                unsafe { &mut *(buf.deref_inner_mut().data.as_mut_ptr() as *mut DImapBlock) };
            if new_addr != 0 {
                // Copy the imap block content from old imap block.
                let mut old_buf = self.get_imap_block(block_no, ctx);
                let old_imap_block = unsafe {
                    &mut *(old_buf.deref_inner_mut().data.as_mut_ptr() as *mut DImapBlock)
                };
                *imap_block = old_imap_block.clone();
                // Update imap mapping.
                self.addr[block_no] = new_addr;
                old_buf.free(ctx);
            }
            // Update entry.
            imap_block.entry[offset] = disk_block_no;
            buf.free(ctx);
            true
        } else {
            false
        }
    }
}
