// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
//! Create 6.0.10 BasinRecipe/BasinInventory/BasinBlockEntity and mixer core.
//! Source ac0c444d9828da3453ae8cc65338e8de063286fb. Item/fluid keys include
//! components; registry predicates, RNG, world IO, NBT and effects stay outside.
//! Deliberately preserves independent output simulations and the fluid reserve
//! quirk. An execution failure may have consumed inputs or inserted outputs:
//! `SourceExecutionFailed` reports that source behavior, never rolls it back.

pub const ITEM_SLOTS: usize = 9;
pub const TANKS_PER_HALF: usize = 2;
pub const TANK_CAPACITY: u32 = 1000;
pub const DEFAULT_MIXER_MINIMUM_RPM: f32 = 30.0;

/// Catnip LerpedFloat.linear/Chaser.EXP used by SmartFluidTankBehaviour.
/// Persisted fields are float; Java promotes them to double for each operation
/// and rounds the result back to float. Force is a serialization concern.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FluidLevel {
    pub previous: f32,
    pub value: f32,
    pub target: f32,
    pub speed: f32,
}
impl FluidLevel {
    pub fn tick(&mut self) {
        self.previous = self.value;
        let value = f64::from(self.value);
        let target = f64::from(self.target);
        self.value = if (target - value).abs() < f64::from(0.00001f32) {
            self.target
        } else {
            (value + (target - value) * f64::from(self.speed)) as f32
        };
    }
    pub fn at(&self, partial: f32) -> f32 {
        let previous = f64::from(self.previous);
        (previous + f64::from(partial) * (f64::from(self.value) - previous)) as f32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item<K> {
    pub key: K,
    pub count: u32,
    pub maximum: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fluid<K> {
    pub key: K,
    pub amount: u32,
}
/// The two IFluidHandler drain overloads. Keys include the complete component patch.
#[derive(Clone, Copy, Debug)]
pub enum FluidRequest<'a, K> {
    Any(u32),
    Exact(&'a Fluid<K>),
}
/// `consumed` is what execution removes (or simulation predicts), which may
/// exceed the returned stack because the original nested CombinedTankWrapper
/// discards unlike fluids during maximum-only drains. No rollback is invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainReport<K> {
    pub returned: Option<Fluid<K>>,
    pub consumed: u32,
    pub source_discarded: u32,
}
impl<K> Default for DrainReport<K> {
    fn default() -> Self {
        Self {
            returned: None,
            consumed: 0,
            source_discarded: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heat {
    None,
    Smouldering,
    Fading,
    Kindled,
    Seething,
}
/// BlockHelper.isNotUnheated: a present campfire lit property takes priority.
pub fn is_not_unheated(campfire: bool, lit: Option<bool>, blaze: Option<Heat>) -> bool {
    if campfire && let Some(lit) = lit {
        return lit;
    }
    blaze != Some(Heat::None)
}
/// BasinBlockEntity.getHeatLevelOf. A blaze property is authoritative even
/// when NONE; a passive heater can provide only SMOULDERING, not recipe heat.
pub fn heat_for_block(
    blaze: Option<Heat>,
    passive: bool,
    campfire: bool,
    lit: Option<bool>,
) -> Heat {
    blaze.unwrap_or_else(|| {
        if passive && is_not_unheated(campfire, lit, blaze) {
            Heat::Smouldering
        } else {
            Heat::None
        }
    })
}
#[derive(Clone, Copy, Debug, Default)]
pub struct HeatCache(Option<Heat>);
impl HeatCache {
    pub fn reset(&mut self) {
        self.0 = None;
    }
    /// An absent level returns NONE without caching it, as the original BE.
    pub fn get_or_read(&mut self, read: impl FnOnce() -> Option<Heat>) -> Heat {
        if let Some(heat) = self.0 {
            return heat;
        }
        match read() {
            Some(heat) => {
                self.0 = Some(heat);
                heat
            }
            None => Heat::None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeatRequirement {
    None,
    Heated,
    Superheated,
}
impl HeatRequirement {
    pub fn accepts(self, heat: Heat) -> bool {
        match self {
            Self::None => true,
            Self::Heated => !matches!(heat, Heat::None | Heat::Smouldering),
            Self::Superheated => heat == Heat::Seething,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeKind {
    Basin,
    Crafting,
}
#[derive(Clone, Debug)]
pub struct Roll<K> {
    pub stack: Item<K>,
    pub chance: f32,
}
#[derive(Clone, Debug)]
pub struct Recipe<I, F> {
    pub kind: RecipeKind,
    pub item_ingredients: usize,
    pub fluid_amounts: Vec<u32>,
    pub outputs: Vec<Roll<I>>,
    pub fluid_outputs: Vec<Fluid<F>>,
    pub heat: HeatRequirement,
    pub duration: i32,
}
/// Predicate tokens remain recipe-specific; an ingredient can accept a tag or
/// ignore components while the full consumed/produced stack retains them.
pub trait Context<I, F> {
    fn item_matches(&mut self, ingredient: usize, stack: &Item<I>) -> bool;
    fn fluid_matches(&mut self, ingredient: usize, stack: &Fluid<F>) -> bool;
    fn remainder(&mut self, _unit: &Item<I>) -> Option<Item<I>> {
        None
    }
    fn random(&mut self) -> f32;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    Internal,
    /// Presence means a real destination capability, not free capacity. Source
    /// spouts append to overflow without simulating destination free space.
    Spout {
        item_destination: bool,
        fluid_destination: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Basin<I, F> {
    pub input: [Option<Item<I>>; ITEM_SLOTS],
    pub output: [Option<Item<I>>; ITEM_SLOTS],
    pub input_fluids: [Option<Fluid<F>>; TANKS_PER_HALF],
    pub output_fluids: [Option<Fluid<F>>; TANKS_PER_HALF],
    pub overflow: Vec<Item<I>>,
    pub fluid_overflow: Vec<Fluid<F>>,
    pub mode: OutputMode,
    pub heat: Heat,
    pub item_capability: bool,
    pub fluid_capability: bool,
    pub valid_block: bool,
}
impl<I, F> Default for Basin<I, F> {
    fn default() -> Self {
        Self {
            input: std::array::from_fn(|_| None),
            output: std::array::from_fn(|_| None),
            input_fluids: std::array::from_fn(|_| None),
            output_fluids: std::array::from_fn(|_| None),
            overflow: Vec::new(),
            fluid_overflow: Vec::new(),
            mode: OutputMode::Internal,
            heat: Heat::None,
            item_capability: true,
            fluid_capability: true,
            valid_block: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    DefinitionBounds,
    MissingItemCapability,
    MissingFluidCapability,
    Heat,
    ItemIngredient(usize),
    FluidIngredient(usize),
    InvalidBlock,
    ItemOutputs,
    FluidOutputs,
    MissingItemDestination,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Filtered,
    Unsupported(Rejection),
    NoMatch(Rejection),
    Simulated,
    Applied,
    SourceExecutionFailed(Rejection),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    pub consumed_items: u32,
    pub consumed_fluid: u32,
    pub inserted_items: u32,
    pub inserted_fluid: u32,
    pub random_calls: u32,
    /// Source invokes both tank behaviours only after every fluid ingredient
    /// succeeded. A later missing ingredient can leave a partial silent drain.
    pub notify_all_fluid_tanks: bool,
}
impl Report {
    fn new(outcome: Outcome) -> Self {
        Self {
            outcome,
            consumed_items: 0,
            consumed_fluid: 0,
            inserted_items: 0,
            inserted_fluid: 0,
            random_calls: 0,
            notify_all_fluid_tanks: false,
        }
    }
    pub fn success(&self) -> bool {
        matches!(self.outcome, Outcome::Simulated | Outcome::Applied)
    }
    pub fn changed(&self) -> bool {
        self.consumed_items != 0
            || self.consumed_fluid != 0
            || self.inserted_items != 0
            || self.inserted_fluid != 0
    }
}
impl<I: Clone + PartialEq, F: Clone + PartialEq> Basin<I, F> {
    pub fn can_continue_processing(&self) -> bool {
        self.overflow.is_empty() && self.fluid_overflow.is_empty()
    }
    /// CombinedInvWrapper is input9, then output9. Recipes can consume either.
    pub fn item(&self, slot: usize) -> Option<&Item<I>> {
        let stack = if slot < ITEM_SLOTS {
            self.input[slot].as_ref()
        } else {
            self.output.get(slot - ITEM_SLOTS)?.as_ref()
        };
        stack.filter(|s| s.count > 0)
    }
    fn item_mut(&mut self, slot: usize) -> &mut Option<Item<I>> {
        if slot < ITEM_SLOTS {
            &mut self.input[slot]
        } else {
            &mut self.output[slot - ITEM_SLOTS]
        }
    }
    /// CombinedTankWrapper is output2, then input2 (opposite item order).
    pub fn fluid(&self, tank: usize) -> Option<&Fluid<F>> {
        let stack = if tank < TANKS_PER_HALF {
            self.output_fluids[tank].as_ref()
        } else {
            self.input_fluids.get(tank - TANKS_PER_HALF)?.as_ref()
        };
        stack.filter(|s| s.amount > 0)
    }
    fn fluid_mut(&mut self, tank: usize) -> &mut Option<Fluid<F>> {
        if tank < TANKS_PER_HALF {
            &mut self.output_fluids[tank]
        } else {
            &mut self.input_fluids[tank - TANKS_PER_HALF]
        }
    }
    /// Basin's output handler forbids insertion. Its input handler's exact
    /// enforceVariety algorithm allows a NEW fluid to span empty tanks in one
    /// call, but a later matching full tank prevents filling its empty sibling.
    pub fn fill_input_fluid(&mut self, fluid: &Fluid<F>, simulate: bool) -> u32 {
        fill_varied(&mut self.input_fluids, fluid, simulate)
    }
    /// Canonical capability order: output half, then input half. Maximum-only
    /// drain must preserve BOTH nested wrapper levels: the outer wrapper
    /// subtracts each inner wrapper's RETURNED amount, not its actual removal.
    pub fn drain_fluid(&mut self, request: FluidRequest<'_, F>, simulate: bool) -> DrainReport<F> {
        let mut result = DrainReport::default();
        let mut remaining = match request {
            FluidRequest::Any(amount) => amount,
            FluidRequest::Exact(fluid) => fluid.amount,
        };
        if remaining == 0 {
            return result;
        }
        let selected = match request {
            FluidRequest::Exact(fluid) => Some(&fluid.key),
            _ => None,
        };
        for half in [&mut self.output_fluids, &mut self.input_fluids] {
            let part = drain_half(half, remaining, selected, simulate);
            let returned_amount = part.returned.as_ref().map_or(0, |fluid| fluid.amount);
            remaining -= returned_amount;
            result.consumed += part.consumed;
            merge_drained(&mut result.returned, part.returned);
            if remaining == 0 {
                break;
            }
        }
        result.source_discarded =
            result.consumed - result.returned.as_ref().map_or(0, |fluid| fluid.amount);
        result
    }
    pub fn insert_input(&mut self, slot: usize, stack: Item<I>, simulate: bool) -> Item<I> {
        if slot >= ITEM_SLOTS {
            return stack;
        }
        insert_slot(&mut self.input, slot, stack, simulate)
    }
    pub fn match_recipe(
        &mut self,
        recipe: &Recipe<I, F>,
        filter_available: bool,
        filter_pass: bool,
        context: &mut impl Context<I, F>,
    ) -> Report {
        if !filter_available || !filter_pass {
            return Report::new(Outcome::Filtered);
        }
        self.apply(recipe, true, context)
    }
    /// The two source passes are not an atomic transaction. Match simulations
    /// do not alter inventory, but they DO roll results and consume RNG.
    pub fn apply(
        &mut self,
        recipe: &Recipe<I, F>,
        test: bool,
        context: &mut impl Context<I, F>,
    ) -> Report {
        let mut report = Report::new(Outcome::Applied);
        if recipe.item_ingredients > 64
            || recipe.outputs.len() > 4
            || recipe.fluid_amounts.len() > 2
            || recipe.fluid_outputs.len() > 2
            || recipe.outputs.iter().any(|r| {
                !r.chance.is_finite()
                    || !(0.0..=1.0).contains(&r.chance)
                    || r.stack.count > 99
                    || r.stack.maximum == 0
            })
            || recipe
                .fluid_amounts
                .iter()
                .any(|a| *a == 0 || *a > i32::MAX as u32)
            || recipe
                .fluid_outputs
                .iter()
                .any(|a| a.amount > i32::MAX as u32)
        {
            report.outcome = Outcome::Unsupported(Rejection::DefinitionBounds);
            return report;
        }
        if !self.item_capability {
            report.outcome = Outcome::NoMatch(Rejection::MissingItemCapability);
            return report;
        }
        if !self.fluid_capability {
            report.outcome = Outcome::NoMatch(Rejection::MissingFluidCapability);
            return report;
        }
        if recipe.kind == RecipeKind::Basin && !recipe.heat.accepts(self.heat) {
            report.outcome = Outcome::NoMatch(Rejection::Heat);
            return report;
        }
        let mut outputs = Vec::new();
        let fluids = if recipe.kind == RecipeKind::Basin {
            recipe
                .fluid_outputs
                .iter()
                .filter(|f| f.amount > 0)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        for simulate in [true, false] {
            if !simulate && test {
                report.outcome = Outcome::Simulated;
                return report;
            }
            let mut extracted_items = [0u32; ITEM_SLOTS * 2];
            let mut extracted_fluids = [0u32; TANKS_PER_HALF * 2];
            for ingredient in 0..recipe.item_ingredients {
                let mut found = false;
                for (slot, extracted) in extracted_items.iter_mut().enumerate() {
                    let Some(stack) = self.item(slot) else {
                        continue;
                    };
                    if stack.count == 0 || simulate && stack.count <= *extracted {
                        continue;
                    }
                    let unit = Item {
                        count: 1,
                        ..stack.clone()
                    };
                    if !context.item_matches(ingredient, &unit) {
                        continue;
                    }
                    if !simulate {
                        let stack = self.item_mut(slot);
                        let item = stack.as_mut().expect("known occupied source slot");
                        item.count -= 1;
                        if item.count == 0 {
                            *stack = None;
                        }
                        report.consumed_items += 1;
                    }
                    *extracted += 1;
                    found = true;
                    break;
                }
                if !found {
                    report.outcome = if simulate {
                        Outcome::NoMatch(Rejection::ItemIngredient(ingredient))
                    } else {
                        Outcome::SourceExecutionFailed(Rejection::ItemIngredient(ingredient))
                    };
                    return report;
                }
            }
            if recipe.kind == RecipeKind::Basin {
                for (ingredient, required) in recipe.fluid_amounts.iter().enumerate() {
                    let mut remaining = *required;
                    let mut found = false;
                    for (tank, extracted) in extracted_fluids.iter_mut().enumerate() {
                        let Some(stack) = self.fluid(tank) else {
                            continue;
                        };
                        if stack.amount == 0 || simulate && stack.amount <= *extracted {
                            continue;
                        }
                        // Actual NeoForge21.1.227 SizedFluidIngredient.test also
                        // requires the full amount, not this pass's remainder.
                        if stack.amount < *required || !context.fluid_matches(ingredient, stack) {
                            continue;
                        }
                        let drained = remaining.min(stack.amount);
                        if !simulate {
                            let slot = self.fluid_mut(tank);
                            let fluid = slot.as_mut().expect("known occupied source tank");
                            fluid.amount -= drained;
                            if fluid.amount == 0 {
                                *slot = None;
                            }
                            report.consumed_fluid += drained;
                        }
                        remaining -= drained;
                        if remaining != 0 {
                            continue;
                        }
                        // Preserve the source's gate-only simulation accounting.
                        // It does not subtract reserved volume from stack.amount.
                        *extracted += drained;
                        found = true;
                        break;
                    }
                    if !found {
                        report.outcome = if simulate {
                            Outcome::NoMatch(Rejection::FluidIngredient(ingredient))
                        } else {
                            Outcome::SourceExecutionFailed(Rejection::FluidIngredient(ingredient))
                        };
                        return report;
                    }
                }
            }
            if !simulate && report.consumed_fluid > 0 {
                report.notify_all_fluid_tanks = true;
            }
            if simulate {
                for roll in &recipe.outputs {
                    let mut count = roll.stack.count;
                    if recipe.kind == RecipeKind::Basin && roll.chance != 1.0 {
                        count = 0;
                        for _ in 0..roll.stack.count {
                            report.random_calls += 1;
                            if context.random() <= roll.chance {
                                count += 1;
                            }
                        }
                    }
                    if count > 0 {
                        outputs.push(Item {
                            count,
                            ..roll.stack.clone()
                        });
                    }
                }
                // DummyCraftingContainer expands consumed units in SLOT order.
                for (slot, count) in extracted_items.into_iter().enumerate() {
                    if let Some(stack) = self.item(slot) {
                        let unit = Item {
                            count: 1,
                            ..stack.clone()
                        };
                        for _ in 0..count {
                            if let Some(remainder) = context.remainder(&unit)
                                && remainder.count > 0
                            {
                                outputs.push(remainder);
                            }
                        }
                    }
                }
            }
            if let Err(reason) = self.accept_outputs(&outputs, &fluids, simulate, &mut report) {
                report.outcome = if simulate {
                    Outcome::NoMatch(reason)
                } else {
                    Outcome::SourceExecutionFailed(reason)
                };
                return report;
            }
        }
        report
    }
    fn accept_outputs(
        &mut self,
        items: &[Item<I>],
        fluids: &[Fluid<F>],
        simulate: bool,
        report: &mut Report,
    ) -> Result<(), Rejection> {
        if !self.valid_block {
            return Err(Rejection::InvalidBlock);
        }
        if let OutputMode::Spout {
            item_destination,
            fluid_destination,
        } = self.mode
        {
            if !items.is_empty() && !item_destination {
                return Err(Rejection::MissingItemDestination);
            }
            if !fluids.is_empty() && !fluid_destination {
                self.accept_fluids(fluids, simulate, report)?;
            }
            if !simulate {
                for item in items {
                    if item.count > 0 {
                        self.overflow.push(item.clone());
                        report.inserted_items += item.count;
                    }
                }
                if fluid_destination {
                    for fluid in fluids {
                        if fluid.amount > 0 {
                            self.fluid_overflow.push(fluid.clone());
                            report.inserted_fluid += fluid.amount;
                        }
                    }
                }
            }
            return Ok(());
        }
        // Every output simulates against the SAME original inventory. Do not
        // reserve previous simulated outputs: this is a known source quirk.
        for item in items {
            let remainder = insert_stacked(&mut self.output, item.clone(), simulate);
            if !simulate {
                report.inserted_items += item.count - remainder.count;
            }
            if remainder.count != 0 {
                return Err(Rejection::ItemOutputs);
            }
        }
        self.accept_fluids(fluids, simulate, report)
    }
    fn accept_fluids(
        &mut self,
        fluids: &[Fluid<F>],
        simulate: bool,
        report: &mut Report,
    ) -> Result<(), Rejection> {
        for fluid in fluids {
            let filled = fill_varied(&mut self.output_fluids, fluid, simulate);
            if !simulate {
                report.inserted_fluid += filled;
            }
            if filled != fluid.amount {
                return Err(Rejection::FluidOutputs);
            }
        }
        Ok(())
    }
}

fn insert_slot<K: Clone + PartialEq>(
    slots: &mut [Option<Item<K>>; ITEM_SLOTS],
    slot: usize,
    stack: Item<K>,
    simulate: bool,
) -> Item<K> {
    if stack.count == 0 {
        return stack;
    }
    let empty = |s: &Option<Item<K>>| s.as_ref().is_none_or(|s| s.count == 0);
    let first_empty = slots.iter().position(empty);
    if slots.iter().enumerate().any(|(i, s)| {
        i != slot
            && s.as_ref()
                .is_some_and(|s| s.count > 0 && s.key == stack.key)
    }) {
        return stack;
    }
    if empty(&slots[slot]) && first_empty != Some(slot) {
        return stack;
    }
    if slots[slot]
        .as_ref()
        .is_some_and(|s| s.count > 0 && s.key != stack.key)
    {
        return stack;
    }
    let current = slots[slot].as_ref().map_or(0, |s| s.count);
    let accepted = stack
        .count
        .min(64.min(stack.maximum).saturating_sub(current));
    if !simulate && accepted > 0 {
        slots[slot] = Some(Item {
            count: current + accepted,
            ..stack.clone()
        });
    }
    Item {
        count: stack.count - accepted,
        ..stack
    }
}
fn insert_stacked<K: Clone + PartialEq>(
    slots: &mut [Option<Item<K>>; ITEM_SLOTS],
    mut stack: Item<K>,
    simulate: bool,
) -> Item<K> {
    if stack.maximum > 1 {
        for slot in 0..ITEM_SLOTS {
            if slots[slot]
                .as_ref()
                .is_some_and(|s| s.count > 0 && s.key == stack.key)
            {
                stack = insert_slot(slots, slot, stack, simulate);
                if stack.count == 0 {
                    return stack;
                }
            }
        }
    }
    for slot in 0..ITEM_SLOTS {
        if slots[slot].as_ref().is_none_or(|s| s.count == 0) {
            stack = insert_slot(slots, slot, stack, simulate);
            if stack.count == 0 {
                break;
            }
        }
    }
    stack
}
fn merge_drained<K: PartialEq>(total: &mut Option<Fluid<K>>, part: Option<Fluid<K>>) {
    if let Some(part) = part {
        match total {
            Some(total) if total.key == part.key => total.amount += part.amount,
            None => *total = Some(part),
            _ => {} // Exact original wrapper: an unlike drained fluid is lost.
        }
    }
}
fn drain_half<K: Clone + PartialEq>(
    tanks: &mut [Option<Fluid<K>>; TANKS_PER_HALF],
    maximum: u32,
    selected: Option<&K>,
    simulate: bool,
) -> DrainReport<K> {
    let mut result = DrainReport::default();
    let mut remaining = maximum;
    for tank in tanks {
        let Some(current) = tank.as_ref().filter(|fluid| fluid.amount > 0) else {
            continue;
        };
        if selected.is_some_and(|key| *key != current.key) {
            continue;
        }
        let amount = remaining.min(current.amount);
        if amount == 0 {
            break;
        }
        let part = Fluid {
            key: current.key.clone(),
            amount,
        };
        if !simulate {
            let left = current.amount - amount;
            *tank = (left > 0).then(|| Fluid {
                amount: left,
                ..part.clone()
            });
        }
        remaining -= amount;
        result.consumed += amount;
        merge_drained(&mut result.returned, Some(part));
        if remaining == 0 {
            break;
        }
    }
    result.source_discarded =
        result.consumed - result.returned.as_ref().map_or(0, |fluid| fluid.amount);
    result
}

fn fill_varied<K: Clone + PartialEq>(
    tanks: &mut [Option<Fluid<K>>; TANKS_PER_HALF],
    fluid: &Fluid<K>,
    simulate: bool,
) -> u32 {
    if fluid.amount == 0 {
        return 0;
    }
    let mut remaining = fluid.amount;
    let mut fitting_handler_found = false;
    // Exact CombinedTankWrapper.fill(enforceVariety=true): a NEW fluid may
    // span both empty tanks. An already matching tank stops the search even
    // when full, so a later matching/empty tank is intentionally not filled.
    'outer: for search_pass in [true, false] {
        for tank in tanks.iter_mut() {
            let same = tank
                .as_ref()
                .is_some_and(|f| f.amount > 0 && f.key == fluid.key);
            if search_pass && same {
                fitting_handler_found = true;
            }
            if search_pass && !fitting_handler_found {
                continue;
            }
            let current = tank.as_ref().map_or(0, |f| f.amount);
            let accepts = current == 0 || same;
            let filled = if accepts {
                remaining.min(TANK_CAPACITY.saturating_sub(current))
            } else {
                0
            };
            if !simulate && filled > 0 {
                *tank = Some(Fluid {
                    amount: current + filled,
                    ..fluid.clone()
                });
            }
            remaining -= filled;
            if remaining == 0 || fitting_handler_found {
                break 'outer;
            }
        }
    }
    fluid.amount - remaining
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mixer {
    pub running: bool,
    pub ticks: i32,
    pub processing_ticks: i32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerEvents {
    pub initialized_processing: bool,
    pub apply_called: bool,
    pub schedule_basin_check: bool,
}
impl Mixer {
    pub fn start(&mut self) -> bool {
        if self.running && self.ticks <= 20 {
            return false;
        }
        self.running = true;
        self.ticks = 0;
        true
    }
    pub fn basin_removed(&mut self) {
        if self.running {
            self.ticks = 40;
            self.running = false;
        }
    }
    pub fn tick(
        &mut self,
        rpm: f32,
        minimum_rpm: f32,
        duration: i32,
        mut apply_and_continue: impl FnMut() -> bool,
    ) -> MixerEvents {
        let mut events = MixerEvents::default();
        if self.ticks >= 40 {
            self.running = false;
            self.ticks = 0;
            events.schedule_basin_check = true;
            return events;
        }
        if !self.running {
            return events;
        }
        let speed = rpm.abs();
        if rpm == 0.0
            || matches!(
                speed.partial_cmp(&minimum_rpm),
                None | Some(std::cmp::Ordering::Less)
            )
        {
            if self.ticks < 20 {
                self.ticks = 40 - self.ticks;
            } else if self.ticks == 20 {
                self.ticks += 1;
            }
        }
        if self.ticks == 20 {
            if self.processing_ticks < 0 {
                self.processing_ticks = processing_time(speed, duration);
                events.initialized_processing = true;
            } else {
                self.processing_ticks -= 1;
                if self.processing_ticks == 0 {
                    self.ticks += 1;
                    self.processing_ticks = -1;
                    events.apply_called = true;
                    if apply_and_continue() {
                        self.ticks = 20;
                    }
                }
            }
        }
        if self.ticks != 20 {
            self.ticks += 1;
        }
        events
    }
    /// The adapter supplies Minecraft's cosine implementation for exact visual
    /// sampling; the portable state machine does not substitute renderer math.
    pub fn rendered_head_offset(self, partial: f32, cos: impl Fn(f32) -> f32) -> f32 {
        let mut offset = 0.0;
        if self.running {
            if self.ticks < 20 {
                let n = (self.ticks as f32 + partial) / 20.0;
                offset = (2.0 - cos((f64::from(n) * std::f64::consts::PI) as f32)) / 2.0 - 0.5;
            } else if self.ticks == 20 {
                offset = 1.0;
            } else {
                let n = ((40 - self.ticks) as f32 - partial) / 20.0;
                offset = (2.0 - cos((f64::from(n) * std::f64::consts::PI) as f32)) / 2.0 - 0.5;
            }
        }
        offset + 7.0 / 16.0
    }
    pub fn rendered_rotation_speed(self, rpm: f32) -> f32 {
        if !self.running {
            rpm / 2.0
        } else if self.ticks < 15 || self.ticks > 20 {
            rpm
        } else {
            rpm * 2.0
        }
    }
}
pub fn processing_time(speed: f32, duration: i32) -> i32 {
    let recipe_speed = if duration == 0 {
        1.0
    } else {
        duration as f32 / 100.0
    };
    let ratio = (512.0 / speed) as i32;
    let log = if ratio <= 0 {
        -1
    } else {
        31 - ratio.leading_zeros() as i32
    };
    // Java integer arithmetic wraps; valid recipe durations are small, but the
    // portable representation also retains that behavior for direct callers.
    log.wrapping_mul((recipe_speed * 15.0).ceil() as i32)
        .wrapping_add(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    fn capability_basin() -> Basin<i32, (&'static str, bool)> {
        Basin {
            output_fluids: [
                Some(Fluid {
                    key: ("water", false),
                    amount: 100,
                }),
                Some(Fluid {
                    key: ("lava", false),
                    amount: 100,
                }),
            ],
            input_fluids: [
                Some(Fluid {
                    key: ("water", false),
                    amount: 200,
                }),
                Some(Fluid {
                    key: ("lava", false),
                    amount: 200,
                }),
            ],
            ..Basin::default()
        }
    }
    #[test]
    fn capability_fill_new_fluid_spans_tanks_but_a_later_matching_full_tank_blocks() {
        let fluid = Fluid {
            key: 1,
            amount: 2000,
        };
        let mut fresh: Basin<i32, i32> = Basin::default();
        let before = fresh.clone();
        assert_eq!(fresh.fill_input_fluid(&fluid, true), 2000);
        assert_eq!(fresh, before);
        assert_eq!(fresh.fill_input_fluid(&fluid, false), 2000);
        assert_eq!(fresh.input_fluids.map(|f| f.unwrap().amount), [1000, 1000]);
        let mut sequential: Basin<i32, i32> = Basin::default();
        let bucket = Fluid {
            amount: 1000,
            ..fluid
        };
        assert_eq!(sequential.fill_input_fluid(&bucket, false), 1000);
        assert_eq!(sequential.fill_input_fluid(&bucket, false), 0);
        assert!(sequential.input_fluids[1].is_none());
    }
    #[test]
    fn capability_selected_drain_is_output_first_and_matches_component_removal() {
        let mut basin = capability_basin();
        let wrong = Fluid {
            key: ("water", true),
            amount: 250,
        };
        let before = basin.clone();
        assert_eq!(
            basin.drain_fluid(FluidRequest::Exact(&wrong), false),
            DrainReport::default()
        );
        assert_eq!(basin, before);
        let water = Fluid {
            key: ("water", false),
            amount: 250,
        };
        let report = basin.drain_fluid(FluidRequest::Exact(&water), false);
        assert_eq!(report.returned, Some(water));
        assert_eq!((report.consumed, report.source_discarded), (250, 0));
        assert!(basin.output_fluids[0].is_none());
        assert_eq!(basin.input_fluids[0].as_ref().unwrap().amount, 50);
        assert_eq!(basin.output_fluids[1], before.output_fluids[1]);
    }
    #[test]
    fn capability_maximum_drain_retains_nested_source_discard_and_simulation_is_read_only() {
        let mut basin = capability_basin();
        let before = basin.clone();
        let predicted = basin.drain_fluid(FluidRequest::Any(250), true);
        assert_eq!(basin, before);
        let executed = basin.drain_fluid(FluidRequest::Any(250), false);
        assert_eq!(predicted, executed);
        assert_eq!(
            executed.returned,
            Some(Fluid {
                key: ("water", false),
                amount: 250
            })
        );
        assert_eq!((executed.consumed, executed.source_discarded), (350, 100));
        assert!(basin.output_fluids.iter().all(Option::is_none));
        assert_eq!(basin.input_fluids[0].as_ref().unwrap().amount, 50);
        assert_eq!(basin.input_fluids[1].as_ref().unwrap().amount, 200);
        let unchanged = basin.clone();
        assert_eq!(
            basin.drain_fluid(FluidRequest::Any(0), false),
            DrainReport::default()
        );
        assert_eq!(basin, unchanged);
    }
    use super::*;
    fn item(key: u8, count: u32) -> Item<u8> {
        Item {
            key,
            count,
            maximum: 64,
        }
    }
    fn fluid(key: u8, amount: u32) -> Fluid<u8> {
        Fluid { key, amount }
    }
    fn recipe(ingredients: usize) -> Recipe<u8, u8> {
        Recipe {
            kind: RecipeKind::Basin,
            item_ingredients: ingredients,
            fluid_amounts: vec![],
            outputs: vec![],
            fluid_outputs: vec![],
            heat: HeatRequirement::None,
            duration: 0,
        }
    }
    #[derive(Default)]
    struct C {
        items: Vec<Vec<u8>>,
        fluids: Vec<u8>,
        rolls: Vec<f32>,
        used: usize,
        remainder_order: Vec<u8>,
    }
    impl Context<u8, u8> for C {
        fn item_matches(&mut self, i: usize, stack: &Item<u8>) -> bool {
            self.items[i].contains(&stack.key)
        }
        fn fluid_matches(&mut self, i: usize, stack: &Fluid<u8>) -> bool {
            self.fluids[i] == stack.key
        }
        fn remainder(&mut self, unit: &Item<u8>) -> Option<Item<u8>> {
            self.remainder_order.push(unit.key);
            (unit.key == 9).then(|| item(10, 1))
        }
        fn random(&mut self) -> f32 {
            let v = self.rolls.get(self.used).copied().unwrap_or(0.5);
            self.used += 1;
            v
        }
    }
    #[test]
    fn greedy_order_does_not_solve_ingredient_assignment() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.input[0] = Some(item(1, 1));
        b.input[1] = Some(item(2, 1));
        let original = b.clone();
        let r = recipe(2);
        let result = b.apply(
            &r,
            false,
            &mut C {
                items: vec![vec![1, 2], vec![1]],
                ..C::default()
            },
        );
        assert_eq!(
            result.outcome,
            Outcome::NoMatch(Rejection::ItemIngredient(1))
        );
        assert_eq!(b, original);
        assert!(
            b.apply(
                &r,
                false,
                &mut C {
                    items: vec![vec![1], vec![1, 2]],
                    ..C::default()
                }
            )
            .success()
        );
        assert!(b.input.iter().all(Option::is_none));
    }
    #[test]
    fn combined_inventory_consumes_output_and_remainders_follow_slots() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.input[0] = Some(item(9, 2));
        b.output[0] = Some(item(2, 1));
        let mut c = C {
            items: vec![vec![2], vec![9], vec![9]],
            ..C::default()
        };
        let result = b.apply(&recipe(3), false, &mut c);
        assert_eq!(result.consumed_items, 3);
        assert_eq!(c.remainder_order, vec![9, 9, 2]);
        assert_eq!(b.output[0], Some(item(10, 2)));
        assert!(result.success());
    }
    #[test]
    fn unique_insertion_and_first_empty_slot_are_component_sensitive() {
        let mut b: Basin<u8, u8> = Basin::default();
        assert_eq!(b.insert_input(1, item(1, 2), false).count, 2);
        assert_eq!(b.insert_input(0, item(1, 60), false).count, 0);
        assert_eq!(b.insert_input(1, item(1, 2), false).count, 2);
        assert_eq!(b.insert_input(0, item(1, 6), true).count, 2);
        assert_eq!(b.input[0], Some(item(1, 60)));
        assert_eq!(b.insert_input(1, item(2, 2), false).count, 0);
    }
    #[test]
    fn simulated_outputs_are_not_reserved_sixty_plus_three_plus_three() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.input[0] = Some(item(1, 1));
        b.output[0] = Some(item(2, 60));
        let mut r = recipe(1);
        r.outputs = vec![
            Roll {
                stack: item(2, 3),
                chance: 1.0
            };
            2
        ];
        let mut c = C {
            items: vec![vec![1]],
            ..C::default()
        };
        let original = b.clone();
        assert!(b.apply(&r, true, &mut c).success());
        assert_eq!(b, original);
        let result = b.apply(&r, false, &mut c);
        assert_eq!(
            result.outcome,
            Outcome::SourceExecutionFailed(Rejection::ItemOutputs)
        );
        assert_eq!((result.consumed_items, result.inserted_items), (1, 4));
        assert!(result.changed());
        assert_eq!(b.output[0], Some(item(2, 64)));
        assert!(b.input[0].is_none());
    }
    #[test]
    fn blocked_output_prevents_initial_consumption() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.input[0] = Some(item(1, 1));
        for i in 0..9 {
            b.output[i] = Some(item(10 + i as u8, 64));
        }
        let mut r = recipe(1);
        r.outputs.push(Roll {
            stack: item(2, 1),
            chance: 1.0,
        });
        let original = b.clone();
        let result = b.apply(
            &r,
            false,
            &mut C {
                items: vec![vec![1]],
                ..C::default()
            },
        );
        assert_eq!(result.outcome, Outcome::NoMatch(Rejection::ItemOutputs));
        assert!(!result.changed());
        assert_eq!(b, original);
    }
    #[test]
    fn fluid_reservation_gate_retains_partial_consumption_failure() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.output_fluids[0] = Some(fluid(1, 500));
        let mut r = recipe(0);
        r.fluid_amounts = vec![300, 300];
        let mut c = C {
            fluids: vec![1, 1],
            ..C::default()
        };
        assert!(b.apply(&r, true, &mut c).success());
        let result = b.apply(&r, false, &mut c);
        assert_eq!(
            result.outcome,
            Outcome::SourceExecutionFailed(Rejection::FluidIngredient(1))
        );
        assert_eq!(result.consumed_fluid, 300);
        assert_eq!(b.output_fluids[0], Some(fluid(1, 200)));
        assert!(!result.notify_all_fluid_tanks);
    }
    #[test]
    fn sized_fluid_predicate_does_not_combine_smaller_tanks() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.output_fluids[0] = Some(fluid(1, 200));
        b.input_fluids[0] = Some(fluid(1, 200));
        let mut r = recipe(0);
        r.fluid_amounts = vec![300];
        let original = b.clone();
        let result = b.apply(
            &r,
            false,
            &mut C {
                fluids: vec![1],
                ..C::default()
            },
        );
        assert_eq!(
            result.outcome,
            Outcome::NoMatch(Rejection::FluidIngredient(0))
        );
        assert_eq!(b, original);
    }
    #[test]
    fn fluid_input_order_is_output_then_input_and_notifies_after_success() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.output_fluids[0] = Some(fluid(1, 200));
        b.input_fluids[0] = Some(fluid(1, 700));
        let mut r = recipe(0);
        r.fluid_amounts = vec![150];
        let result = b.apply(
            &r,
            false,
            &mut C {
                fluids: vec![1],
                ..C::default()
            },
        );
        assert_eq!(b.output_fluids[0], Some(fluid(1, 50)));
        assert_eq!(b.input_fluids[0], Some(fluid(1, 700)));
        assert!(result.success() && result.notify_all_fluid_tanks);
    }
    #[test]
    fn simulated_fluid_outputs_can_overbook_capacity() {
        let mut b: Basin<u8, u8> = Basin::default();
        b.output_fluids[0] = Some(fluid(1, 700));
        let mut r = recipe(0);
        r.fluid_outputs = vec![fluid(1, 200), fluid(1, 200)];
        let result = b.apply(&r, false, &mut C::default());
        assert_eq!(
            result.outcome,
            Outcome::SourceExecutionFailed(Rejection::FluidOutputs)
        );
        assert_eq!(result.inserted_fluid, 300);
        assert_eq!(b.output_fluids[0], Some(fluid(1, 1000)));
    }
    #[test]
    fn variety_new_fluid_spans_tanks_but_existing_full_match_stops_search() {
        let mut b: Basin<u8, u8> = Basin::default();
        let mut r = recipe(0);
        r.fluid_outputs = vec![fluid(1, 1500)];
        assert!(b.apply(&r, false, &mut C::default()).success());
        assert_eq!(b.output_fluids, [Some(fluid(1, 1000)), Some(fluid(1, 500))]);
        r.fluid_outputs = vec![fluid(1, 100)];
        let original = b.clone();
        assert_eq!(
            b.apply(&r, false, &mut C::default()).outcome,
            Outcome::NoMatch(Rejection::FluidOutputs)
        );
        assert_eq!(b, original);
    }
    #[test]
    fn match_rolls_rng_without_mutation_and_preserves_inclusive_probability() {
        let mut b: Basin<u8, u8> = Basin::default();
        let mut r = recipe(0);
        r.outputs = vec![
            Roll {
                stack: item(1, 2),
                chance: 0.0,
            },
            Roll {
                stack: item(2, 3),
                chance: 1.0,
            },
        ];
        let mut c = C {
            rolls: vec![0.0, 0.5, 0.5, 0.5],
            ..C::default()
        };
        let original = b.clone();
        assert_eq!(
            b.match_recipe(&r, false, true, &mut c).outcome,
            Outcome::Filtered
        );
        assert_eq!(c.used, 0);
        assert_eq!(b.match_recipe(&r, true, true, &mut c).random_calls, 2);
        assert_eq!(b, original);
        assert_eq!(c.used, 2);
        assert_eq!(b.apply(&r, false, &mut c).random_calls, 2);
        assert!(
            b.output
                .iter()
                .all(|s| s.as_ref().is_none_or(|s| s.key != 1))
        );
        let mut c = C {
            rolls: vec![0.0, 0.5],
            ..C::default()
        };
        let mut b: Basin<u8, u8> = Basin::default();
        b.apply(&r, false, &mut c);
        assert_eq!(b.output[0], Some(item(1, 1)));
        assert_eq!(b.output[1], Some(item(2, 3)));
    }
    #[test]
    fn spout_checks_presence_and_queues_without_destination_capacity_simulation() {
        let mut b: Basin<u8, u8> = Basin {
            mode: OutputMode::Spout {
                item_destination: true,
                fluid_destination: false,
            },
            ..Basin::default()
        };
        let mut r = recipe(0);
        r.outputs = vec![Roll {
            stack: item(1, 64),
            chance: 1.0,
        }];
        r.fluid_outputs = vec![fluid(1, 100)];
        assert!(b.apply(&r, false, &mut C::default()).success());
        assert_eq!(b.overflow, vec![item(1, 64)]);
        assert_eq!(b.output_fluids[0], Some(fluid(1, 100)));
        assert!(!b.can_continue_processing());
        b.overflow.clear();
        assert!(b.can_continue_processing());
        b.mode = OutputMode::Spout {
            item_destination: false,
            fluid_destination: true,
        };
        assert_eq!(
            b.apply(&r, false, &mut C::default()).outcome,
            Outcome::NoMatch(Rejection::MissingItemDestination)
        );
    }
    #[test]
    fn heat_and_both_capabilities_are_required_before_any_consumption() {
        for heat in [
            Heat::None,
            Heat::Smouldering,
            Heat::Fading,
            Heat::Kindled,
            Heat::Seething,
        ] {
            assert!(HeatRequirement::None.accepts(heat));
            assert_eq!(
                HeatRequirement::Heated.accepts(heat),
                !matches!(heat, Heat::None | Heat::Smouldering)
            );
            assert_eq!(
                HeatRequirement::Superheated.accepts(heat),
                heat == Heat::Seething
            );
        }
        let mut b: Basin<u8, u8> = Basin {
            fluid_capability: false,
            ..Basin::default()
        };
        assert_eq!(
            b.apply(&recipe(0), false, &mut C::default()).outcome,
            Outcome::NoMatch(Rejection::MissingFluidCapability)
        );
        b.fluid_capability = true;
        let mut r = recipe(0);
        r.heat = HeatRequirement::Heated;
        assert_eq!(
            b.apply(&r, false, &mut C::default()).outcome,
            Outcome::NoMatch(Rejection::Heat)
        );
        r.kind = RecipeKind::Crafting;
        assert!(b.apply(&r, false, &mut C::default()).success());
    }
    #[test]
    fn mixer_default_first_batch_repeats_and_retraction_ticks() {
        assert_eq!(
            [30.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0].map(|r| processing_time(r, 0)),
            [61, 61, 46, 31, 16, 1, 1]
        );
        let mut m = Mixer::default();
        assert!(m.start());
        let mut applied = vec![];
        let mut reset = 0;
        for tick in 1..=102 {
            let e = m.tick(32.0, 30.0, 0, || false);
            if e.apply_called {
                applied.push(tick);
                assert_eq!(m.ticks, 22);
            }
            if e.schedule_basin_check {
                reset = tick;
            }
        }
        assert_eq!(applied, vec![83]);
        assert_eq!(reset, 102);
        assert_eq!(
            m,
            Mixer {
                running: false,
                ticks: 0,
                processing_ticks: -1
            }
        );
        let mut m = Mixer::default();
        m.start();
        let mut applied = vec![];
        for tick in 1..=145 {
            if m.tick(32.0, 30.0, 0, || true).apply_called {
                applied.push(tick);
                assert_eq!(m.ticks, 20);
            }
        }
        assert_eq!(applied, vec![83, 145]);
    }
    #[test]
    fn mixer_power_abort_does_not_apply_and_removed_basin_resets() {
        let mut m = Mixer::default();
        m.start();
        for _ in 0..10 {
            m.tick(32.0, 30.0, 100, || panic!("early apply"));
        }
        m.tick(29.0, 30.0, 100, || panic!("abort apply"));
        assert_eq!(m.ticks, 31);
        m.basin_removed();
        assert_eq!((m.running, m.ticks), (false, 40));
        assert!(m.tick(0.0, 30.0, 0, || false).schedule_basin_check);
        let mut m = Mixer {
            running: true,
            ticks: 20,
            processing_ticks: 1,
        };
        m.tick(0.0, 30.0, 100, || panic!("unpowered apply"));
        assert_eq!(m.ticks, 22);
        assert_eq!(m.processing_ticks, 1);
        let m = Mixer::default();
        assert_eq!(m.rendered_head_offset(0.0, f32::cos), 7.0 / 16.0);
        assert_eq!(m.rendered_rotation_speed(32.0), 16.0);
    }
}
