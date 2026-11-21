//! Canonical Huffman decoding (`struct huffman_code`, `create_code`,
//! `add_value`, `make_table`, `read_next_symbol`).

use crate::error::Error;
use crate::stream::Stream;
use std::ops::Deref;

#[derive(Debug, Clone, Copy)]
struct Node {
    branches: [i32; 2],
}

#[derive(Debug, Clone, Copy, Default)]
struct TableEntry {
    length: i32,
    value: i32,
}

#[derive(Debug, Default)]
pub(crate) struct HuffmanCode {
    tree: Vec<Node>,
    minlength: i32,
    maxlength: i32,
    tablesize: i32,
    table: Vec<TableEntry>,
}

/// Why a symbol read failed; maps to libarchive's truncated/bad-data paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymError {
    /// Input ended mid-symbol (`rar->valid = 0` in C).
    Truncated,
    /// Malformed code tables or stream.
    BadCode,
}

fn new_node(code: &mut HuffmanCode) -> Result<usize, Error> {
    if code.tree.len() >= i32::MAX as usize {
        return Err(Error::InvalidData("huffman tree overflow"));
    }
    code.tree.push(Node { branches: [-1, -2] });
    Ok(code.tree.len() - 1)
}

/// `add_value`: insert `value` with code word `codebits`/`length` into the tree.
fn add_value(code: &mut HuffmanCode, value: i32, codebits: i32, length: i32) -> Result<(), Error> {
    // Any previously built table is invalidated.
    code.table = Vec::new();

    if length > code.maxlength {
        code.maxlength = length;
    }
    if length < code.minlength {
        code.minlength = length;
    }

    let mut lastnode = 0usize;
    for bitpos in (0..length).rev() {
        let bit = ((codebits >> bitpos) & 1) as usize;

        // Leaf node check.
        if code.tree[lastnode].branches[0] == code.tree[lastnode].branches[1] {
            return Err(Error::InvalidData("prefix found"));
        }

        // Open branch check.
        if code.tree[lastnode].branches[bit] < 0 {
            let n = new_node(code)?;
            code.tree[lastnode].branches[bit] = n as i32;
        }

        lastnode = code.tree[lastnode].branches[bit] as usize;
    }

    if !(code.tree[lastnode].branches[0] == -1 && code.tree[lastnode].branches[1] == -2) {
        return Err(Error::InvalidData("prefix found"));
    }

    code.tree[lastnode].branches[0] = value;
    code.tree[lastnode].branches[1] = value;
    Ok(())
}

/// `create_code`: build the decode tree from canonical code lengths.
pub(crate) fn create_code(
    code: &mut HuffmanCode,
    lengths: &[u8],
    maxlength: u8,
) -> Result<(), Error> {
    let numsymbols = lengths.len();
    *code = HuffmanCode::default();
    new_node(code)?;
    code.minlength = i32::MAX;
    code.maxlength = i32::MIN;
    let mut codebits = 0i32;
    let mut symbolsleft = numsymbols as i32;
    for i in 1..=(maxlength as i32) {
        for (j, &l) in lengths.iter().enumerate() {
            if l as i32 != i {
                continue;
            }
            add_value(code, j as i32, codebits, i)?;
            codebits += 1;
            symbolsleft -= 1;
            if symbolsleft <= 0 {
                break;
            }
        }
        if symbolsleft <= 0 {
            break;
        }
        codebits <<= 1;
    }
    Ok(())
}

fn make_table(code: &mut HuffmanCode) -> Result<(), Error> {
    code.tablesize = if code.maxlength < code.minlength || code.maxlength > 10 {
        10
    } else {
        code.maxlength
    };
    code.table = vec![TableEntry::default(); 1usize << code.tablesize];
    let mut table = std::mem::take(&mut code.table);
    let r = make_table_recurse(&code.tree, &mut table, 0, 0, code.tablesize);
    code.table = table;
    r
}

/// `make_table_recurse`: fill the flat decode table for the subtree rooted at
/// `node`. `table` is the slice this subtree covers (`currtablesize` entries).
fn make_table_recurse(
    tree: &[Node],
    table: &mut [TableEntry],
    node: i32,
    depth: i32,
    maxdepth: i32,
) -> Result<(), Error> {
    if tree.is_empty() {
        return Err(Error::InvalidData("huffman tree was not created"));
    }
    if node < 0 || node as usize >= tree.len() {
        return Err(Error::InvalidData("invalid huffman tree location"));
    }

    let currtablesize = 1usize << (maxdepth - depth);

    if tree[node as usize].branches[0] == tree[node as usize].branches[1] {
        for e in table.iter_mut() {
            e.length = depth;
            e.value = tree[node as usize].branches[0];
        }
    } else if depth == maxdepth {
        table[0].length = maxdepth + 1;
        table[0].value = node;
    } else {
        make_table_recurse(
            tree,
            &mut table[..currtablesize / 2],
            tree[node as usize].branches[0],
            depth + 1,
            maxdepth,
        )?;
        make_table_recurse(
            tree,
            &mut table[currtablesize / 2..],
            tree[node as usize].branches[1],
            depth + 1,
            maxdepth,
        )?;
    }
    Ok(())
}

impl HuffmanCode {
    /// `read_next_symbol`: decode one symbol from the bit stream.
    pub fn read_next_symbol<B: Deref<Target = [u8]>>(
        &mut self,
        stream: &mut Stream<B>,
    ) -> Result<i32, SymError> {
        if self.table.is_empty() {
            make_table(self).map_err(|_| SymError::BadCode)?;
        }

        if !stream.br_read_ahead(self.tablesize) {
            return Err(SymError::Truncated);
        }
        let bits = stream.br_bits(self.tablesize) as usize;

        let length = self.table[bits].length;
        let value = self.table[bits].value;

        if length < 0 {
            return Err(SymError::BadCode);
        }
        if length <= self.tablesize {
            stream.br_consume(length);
            return Ok(value);
        }

        stream.br_consume(self.tablesize);

        let mut node = value as usize;
        while self.tree[node].branches[0] != self.tree[node].branches[1] {
            if !stream.br_read_ahead(1) {
                return Err(SymError::Truncated);
            }
            let bit = stream.br_bits(1) as usize;
            stream.br_consume(1);
            if self.tree[node].branches[bit] < 0 {
                return Err(SymError::BadCode);
            }
            node = self.tree[node].branches[bit] as usize;
        }
        Ok(self.tree[node].branches[0])
    }
}
