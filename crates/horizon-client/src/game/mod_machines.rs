//! Code mods' block entities (mod API 0.4): the game keeps their slots and
//! the block's properties, lends them to the mod for each tick, and keeps the
//! bytes the mod saves beside the slots in the chunk (`BlockEntity::Mod`).
use super::Game;
#[cfg(feature = "mods")]
use horizon_client::{containers::BlockEntity, mod_machines};

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn tick_mod_machines(&mut self, _machines: &[horizon_client::block_entities::Pos]) {}
}

/// What `machine` asks of the game's items and recipes, read from the
/// resources when a mod calls.
#[cfg(feature = "mods")]
struct GameFacts(std::sync::Arc<super::Resources>);

#[cfg(feature = "mods")]
fn stack_of(id: &str) -> horizon_client::items::ItemStack {
    horizon_client::items::ItemStack {
        item: std::sync::Arc::from(id),
        count: 1,
        damage: 0,
        extra: None,
    }
}

#[cfg(feature = "mods")]
impl horizon_mods::ItemFacts for GameFacts {
    fn max_stack_size(&self, id: &str) -> u32 {
        self.0.items().get(id).map_or(0, |i| i.max_stack)
    }
    fn burn_time(&self, id: &str) -> u32 {
        let stack = stack_of(id);
        self.0
            .providers()
            .burn_time(self.0.items(), &stack, false)
            .max(0) as u32
    }
    fn remainder(&self, id: &str) -> Option<String> {
        horizon_client::containers::remainder(id).map(str::to_owned)
    }
    fn cooking_recipe(
        &self,
        kind: horizon_mods::CookingKind,
        input: &str,
    ) -> Option<horizon_mods::CookingRecipe> {
        use horizon_mods::CookingKind as K;
        let recipe_type = match kind {
            K::Smelting => "minecraft:smelting",
            K::Blasting => "minecraft:blasting",
            K::Smoking => "minecraft:smoking",
            K::Campfire => "minecraft:campfire_cooking",
        };
        let c = self.0.recipes().cook(recipe_type, &stack_of(input))?;
        Some(horizon_mods::CookingRecipe {
            output: horizon_mods::Stack {
                id: c.result.item.to_string(),
                count: c.result.count,
            },
            cooking_time: c.time,
            experience: c.experience,
        })
    }
}

/// How often each machine's saved bytes are read back from its mod, in ticks:
/// what the chunk keeps of the mod's state is at most this old.
#[cfg(feature = "mods")]
const SAVE_EVERY: i64 = 20;
/// How often the runtime's handles are checked against the world's machines.
#[cfg(feature = "mods")]
const SWEEP_EVERY: i64 = 200;

#[cfg(feature = "mods")]
impl Game<'_> {
    /// Tells the lib which blocks have a mod block entity, and the mods the
    /// game's item facts. Called when the mods (re)start, before chunks load.
    pub(super) fn register_mod_machines(&mut self) {
        let types: Vec<_> = self
            .mods
            .mods
            .block_entity_types()
            .map(|t| (t.block.clone(), t.slots as usize, t.ticks))
            .collect();
        // The values each property of those blocks allows: the registry only
        // lists whole states, so one pass reads them off.
        self.mods.machine_props.clear();
        if !types.is_empty() {
            let registry = self.pack.assets.registry;
            for i in 0..registry.len() {
                let Some(state) = registry.state(horizon_world::registry::BlockStateId(i as u32))
                else {
                    continue;
                };
                if !types.iter().any(|(b, _, _)| *b == state.name) {
                    continue;
                }
                let props = self
                    .mods
                    .machine_props
                    .entry(state.name.clone())
                    .or_default();
                for (name, value) in &state.properties {
                    match props.iter_mut().find(|(n, _)| n == name) {
                        Some((_, values)) => {
                            if !values.contains(value) {
                                values.push(value.clone());
                            }
                        }
                        None => props.push((name.clone(), vec![value.clone()])),
                    }
                }
            }
            self.mods
                .mods
                .set_item_facts(std::sync::Arc::new(GameFacts(self.res.clone())));
        }
        mod_machines::set(types);
    }

    /// One tick of the mod block entities at `machines` (those in ticking
    /// chunks): created in their mod when new, lent to it all together, and
    /// whatever it wrote to their slots and block states taken back.
    pub(super) fn tick_mod_machines(&mut self, machines: &[horizon_client::block_entities::Pos]) {
        use horizon_mods::{BlockEntityView, Property, Stack as MachineStack};
        let time = self.world.game_time;
        if time % SWEEP_EVERY == 0 {
            self.sweep_mod_machines();
        }
        if machines.is_empty() {
            return;
        }
        let registry = self.pack.assets.registry;
        // What was lent, to put back only what the mod changed.
        let mut lent = Vec::new();
        let mut views = Vec::new();
        for &pos in machines {
            let state = self.world.block(pos.0, pos.1, pos.2);
            let block = registry.state(state);
            let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos) else {
                continue;
            };
            if m.handle == 0 || !self.mods.mods.has_block_entity(m.handle) {
                // New, or its mod restarted: made from what the chunk saved.
                match self.mods.mods.create_block_entity(m.block, &m.saved) {
                    Some((handle, _)) => m.handle = handle,
                    None => {
                        m.handle = 0;
                        continue;
                    }
                }
            }
            let properties: Vec<Property> = self
                .mods
                .machine_props
                .get(m.block)
                .into_iter()
                .flatten()
                .map(|(name, values)| Property {
                    name: name.clone(),
                    value: block
                        .and_then(|s| s.properties.get(name))
                        .cloned()
                        .unwrap_or_default(),
                    values: values.clone(),
                })
                .collect();
            let slots: Vec<Option<MachineStack>> = m
                .slots
                .iter()
                .map(|s| {
                    s.as_ref().map(|s| MachineStack {
                        id: s.item.to_string(),
                        count: s.count,
                    })
                })
                .collect();
            lent.push((pos, slots.clone(), properties.clone()));
            views.push(BlockEntityView {
                handle: m.handle,
                slots,
                properties,
                changed: false,
            });
        }
        self.mods.mods.tick_block_entities(&mut views);
        for ((pos, old_slots, old_props), view) in lent.into_iter().zip(views) {
            if !view.changed {
                continue;
            }
            if view.slots != old_slots
                && let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos)
            {
                for (i, (new, old)) in view.slots.iter().zip(&old_slots).enumerate() {
                    // A stack the mod left alone keeps its components.
                    if new == old || i >= m.slots.len() {
                        continue;
                    }
                    m.slots[i] = new.as_ref().map(|s| horizon_client::items::ItemStack {
                        count: s.count,
                        ..stack_of(&s.id)
                    });
                }
            }
            if view.properties != old_props {
                self.set_machine_properties(pos, &view.properties);
            }
        }
        // What the chunk keeps of each mod's state follows it closely.
        for &pos in machines {
            let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos) else {
                continue;
            };
            if m.handle != 0
                && (time + i64::from(m.handle)) % SAVE_EVERY == 0
                && let Some(bytes) = self.mods.mods.save_block_entity(m.handle)
            {
                m.saved = bytes;
            }
        }
    }

    /// The block at `pos` with the properties a mod set (the game checked
    /// their values against the block's own).
    fn set_machine_properties(
        &mut self,
        pos: horizon_client::block_entities::Pos,
        props: &[horizon_mods::Property],
    ) {
        let registry = self.pack.assets.registry;
        let state = self.world.block(pos.0, pos.1, pos.2);
        let Some(mut s) = registry.state(state).cloned() else {
            return;
        };
        for p in props {
            if s.properties.contains_key(&p.name) {
                s.properties.insert(p.name.clone(), p.value.clone());
            }
        }
        if let Ok(id) = registry.resolve(&s) {
            let _ = self.world.set(pos.0, pos.1, pos.2, id);
        }
    }

    /// Lets the mods drop the block entities whose block was broken or whose
    /// chunk unloaded: anything the runtime holds that no loaded machine has.
    fn sweep_mod_machines(&mut self) {
        let alive: std::collections::HashSet<u32> = self
            .world
            .block_entities
            .iter_mut()
            .filter_map(|(_, e)| match e {
                BlockEntity::Mod(m) if m.handle != 0 => Some(m.handle),
                _ => None,
            })
            .collect();
        for handle in self.mods.mods.block_entity_handles() {
            if !alive.contains(&handle) {
                self.mods.mods.remove_block_entity(handle);
            }
        }
    }
}
