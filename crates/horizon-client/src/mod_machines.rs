//! The block entities code mods own (mod API 0.4): which blocks have one and
//! how many slots it keeps. The game fills this when its mods start, so
//! `BlockEntity::for_block` (a chunk loading, a block placed) can make a
//! `ModMachine` for a mod's block without the lib knowing the mod runtime.
use std::sync::RwLock;

/// One mod block's block entity: the block, its slot count and whether the
/// mod ticks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineType {
    /// The block's id, `<mod>:<path>`; also the block entity's saved id.
    pub block: &'static str,
    pub slots: usize,
    pub ticks: bool,
}

static TYPES: RwLock<Vec<MachineType>> = RwLock::new(Vec::new());

/// Replaces the registered types with `types` (a mod reload makes new ones).
/// Block ids are kept for the life of the process: a mod's blocks are few
/// and a reload names the same ones.
pub fn set(types: impl IntoIterator<Item = (String, usize, bool)>) {
    let mut known = TYPES.write().unwrap_or_else(|e| e.into_inner());
    let old = std::mem::take(&mut *known);
    for (block, slots, ticks) in types {
        let block = old
            .iter()
            .find(|t| t.block == block)
            .map_or_else(|| &*Box::leak(block.into_boxed_str()), |t| t.block);
        known.push(MachineType {
            block,
            slots,
            ticks,
        });
    }
}

/// The type registered for `block`.
pub fn lookup(block: &str) -> Option<MachineType> {
    let known = TYPES.read().unwrap_or_else(|e| e.into_inner());
    known.iter().find(|t| t.block == block).copied()
}

/// The vanilla menu that shows a machine of `slots` slots (a chest-like grid,
/// the hopper's row or the dispenser's square), none for other counts.
pub fn menu_for(slots: usize) -> Option<&'static str> {
    const ROWS: [&str; 6] = [
        "minecraft:generic_9x1",
        "minecraft:generic_9x2",
        "minecraft:generic_9x3",
        "minecraft:generic_9x4",
        "minecraft:generic_9x5",
        "minecraft:generic_9x6",
    ];
    match slots {
        5 => Some("minecraft:hopper"),
        9 => Some("minecraft:generic_3x3"),
        n if n % 9 == 0 && n <= 54 => Some(ROWS[n / 9 - 1]),
        _ => None,
    }
}

/// A machine screen's title: the block's name key (`block.<mod>.<path>`).
pub fn title_key(block: &str) -> String {
    format!("block.{}", block.replace(':', "."))
}

/// Tests share the one registry: they take this to run one at a time.
#[cfg(test)]
pub static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menus_by_slot_count() {
        assert_eq!(menu_for(5), Some("minecraft:hopper"));
        assert_eq!(menu_for(9), Some("minecraft:generic_3x3"));
        assert_eq!(menu_for(18), Some("minecraft:generic_9x2"));
        assert_eq!(menu_for(54), Some("minecraft:generic_9x6"));
        assert_eq!((menu_for(3), menu_for(0), menu_for(63)), (None, None, None));
        assert_eq!(title_key("my_mod:press"), "block.my_mod.press");
    }
    #[test]
    fn registers_and_replaces() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set([("test_mod:press".to_owned(), 3, true)]);
        let t = lookup("test_mod:press").unwrap();
        assert_eq!((t.slots, t.ticks), (3, true));
        assert!(lookup("test_mod:other").is_none());
        set([("test_mod:press".to_owned(), 5, false)]);
        let again = lookup("test_mod:press").unwrap();
        assert_eq!((again.slots, again.ticks), (5, false));
        assert!(std::ptr::eq(t.block, again.block));
        set([]);
        assert!(lookup("test_mod:press").is_none());
    }
}
