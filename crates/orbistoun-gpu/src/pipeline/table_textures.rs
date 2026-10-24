//! A table slot's binding worked out once a preparation for each table it is read from.
//!
//! What a slot whose descriptor lies in a table binds depends on the slot and on the table's
//! address alone - the user-data words form the address, and guest memory does not change while a
//! submission is prepared. The texture itself is kept once a submission by its descriptor's
//! address, but finding that address and checking the descriptor read guest memory again for
//! every draw, through reads that settle what is pending over them; a frame binds the same tables
//! draw after draw.

use std::collections::HashMap;

use super::{
    GuestMemory, NamedTextures, RenderCommand, StencilPlane, TexelCache, TextureSource,
    TexturesRead, table_address, table_source_bound,
};

/// The answers a preparation has worked out, by the slot's source and its table's address.
#[derive(Default)]
pub(super) struct TableBindings(HashMap<(usize, u64), Result<Option<RenderCommand>, String>>);

impl TableBindings {
    /// [`table_source_bound`], answered from an earlier draw of this preparation that read the
    /// same source from the same table. The stencil plane is the preparation's, the same for every
    /// draw.
    pub(super) fn bound(
        &mut self,
        source: &TextureSource,
        (user, stencil): (&[u32], Option<StencilPlane>),
        (memory, texels): (&impl GuestMemory, &mut TexelCache),
        (read, named): (&mut TexturesRead, &mut NamedTextures),
    ) -> Result<Option<RenderCommand>, String> {
        let key = (
            std::ptr::from_ref(source).addr(),
            table_address(source.table, user),
        );
        if let Some(answer) = self.0.get(&key) {
            return answer.clone();
        }
        let answer = table_source_bound(source, (user, stencil), (memory, texels), (read, named));
        self.0.insert(key, answer.clone());
        answer
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::super::{
        GuestMemory, NamedTextures, RenderCommand, TableBase, TableWord, TexelCache, TextureSource,
        TexturesRead,
    };
    use super::TableBindings;

    /// Tables at 0x1000 and 0x1800, each holding a null image descriptor; counting reads.
    struct Tables {
        bytes: Vec<u8>,
        reads: Cell<usize>,
    }

    impl GuestMemory for Tables {
        fn read(&self, address: u64, length: usize) -> Option<&[u8]> {
            self.reads.set(self.reads.get() + 1);
            let start = usize::try_from(address.checked_sub(0x1000)?).ok()?;
            self.bytes.get(start..start.checked_add(length)?)
        }
    }

    fn source() -> TextureSource {
        TextureSource {
            slot: 0,
            table_offset: Some(0),
            table: TableBase {
                low: TableWord::UserData(0),
                high: TableWord::UserData(1),
            },
            sampler_offset: None,
            user_data: None,
            sampler_user_data: None,
            cube: false,
            volume: false,
            compare: false,
            saturated: [false; 2],
            mirrored: [false; 2],
            shader_compare: false,
            level_user_data: None,
            buffer_offset: None,
            layered: false,
        }
    }

    /// A later draw reading the slot from the same table is answered without reading guest memory,
    /// with what the first draw bound; a draw reading another table reads it.
    #[test]
    fn a_table_slot_is_bound_once_for_the_same_table() {
        let memory = Tables {
            bytes: vec![0; 0x2000],
            reads: Cell::new(0),
        };
        let source = source();
        let (mut texels, mut read, mut named) = (
            TexelCache::new(),
            TexturesRead::new(),
            NamedTextures::default(),
        );
        let mut tables = TableBindings::default();
        let mut bind = |tables: &mut TableBindings, user: &[u32]| {
            tables.bound(
                &source,
                (user, None),
                (&memory, &mut texels),
                (&mut read, &mut named),
            )
        };
        let first = bind(&mut tables, &[0x1000, 0, 7]);
        assert!(
            matches!(first, Ok(Some(RenderCommand::BindTexture { .. }))),
            "{first:?}"
        );
        let after_first = memory.reads.get();
        assert!(after_first > 0);

        let again = bind(&mut tables, &[0x1000, 0, 9]);
        assert_eq!(again, first, "another word changes nothing");
        assert_eq!(memory.reads.get(), after_first, "and nothing is read");

        let _ = bind(&mut tables, &[0x1800, 0, 7]);
        assert!(memory.reads.get() > after_first, "another table is read");
    }
}
