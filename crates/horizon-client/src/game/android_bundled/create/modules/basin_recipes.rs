// Copyright (c) The Create Team / The Creators of Create
// Copyright (c) 2026 create-horizon contributors
// SPDX-License-Identifier: MIT
// BasinOperatingBlockEntity/BasinRecipe and the Mixer/Press static recipe filters.
// Create ac0c444d9828da3453ae8cc65338e8de063286fb (6.0.10).
// The two operators keep distinct source caches; live cycles own their recipe Arc
// across a data-pack reload, while every subsequent lookup sees the new generation.
// Adapted for Android bundled port.

use super::{Game, create_port_basin as basin, create_port_fluid_codec as fluids};
use crate::items::ItemStack;
use android_bundled::create::crates::engine::{basin as rules, transport::SessionCache};
use horizon_client::crafting::{Ingredient, Recipe as Crafting};
use horizon_core::random::RandomSource;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    sync::Arc,
};
type Pos = [i32; 3];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Operator {
    Mixing,
    Compacting,
}
impl Operator {
    fn recipe_type(self) -> &'static str {
        match self {
            Self::Mixing => "create:mixing",
            Self::Compacting => "create:compacting",
        }
    }
}
#[derive(Clone)]
enum Input {
    Item(Ingredient),
    BlockTag(String),
}
#[derive(Clone)]
pub(super) struct Recipe {
    pub(super) id: String,
    inputs: Vec<Input>,
    fluids: Vec<String>,
    pub(super) core: rules::Recipe<basin::Key, fluids::Key>,
}
#[derive(Default)]
struct Recipes {
    generation: u64,
    source: Vec<Arc<serde_json::Value>>,
    vanilla: usize,
    compiled: Vec<Arc<Recipe>>,
}

fn compile(game: &Game<'_>, id: &str, json: &serde_json::Value) -> Option<Recipe> {
    // Strict bounded codecs: no unknown ingredient fields can be dropped to
    // turn an unsupported predicate into a simpler item/fluid match.
    let heat = match json.get("heat_requirement") {
        None => rules::HeatRequirement::None,
        Some(v) => match v.as_str()? {
            "none" => rules::HeatRequirement::None,
            "heated" => rules::HeatRequirement::Heated,
            "superheated" => rules::HeatRequirement::Superheated,
            _ => return None,
        },
    };
    if json["results"]
        .as_array()?
        .iter()
        .any(|i| i.get("fluid").is_some() || i.get("amount").is_some())
    {
        return None;
    }
    let mut inputs = Vec::new();
    let mut fluids = Vec::new();
    let mut fluid_amounts = Vec::new();
    for i in json["ingredients"].as_array()? {
        match i.get("type").and_then(serde_json::Value::as_str) {
            Some("neoforge:single") => {
                if i.as_object()?.len() != 3 {
                    return None;
                }
                let fluid = i["fluid"].as_str()?;
                let amount = i["amount"].as_u64()?;
                if !matches!(fluid, "minecraft:water" | "minecraft:lava")
                    || !(1..=i32::MAX as u64).contains(&amount)
                {
                    return None;
                }
                fluids.push(fluid.to_owned());
                fluid_amounts.push(amount as u32);
            }
            Some("neoforge:block_tag") => {
                if i.as_object()?.len() != 2 {
                    return None;
                }
                inputs.push(Input::BlockTag(i["tag"].as_str()?.to_owned()));
            }
            Some(_) => return None,
            None => inputs.push(Input::Item(
                game.native_recipe_ingredient(&plain_ingredient(i)?)?,
            )),
        }
    }
    let outputs: Option<Vec<_>> = json["results"]
        .as_array()?
        .iter()
        .map(|r| {
            let fields = r.as_object()?;
            if fields.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "id" | "item" | "count" | "chance" | "components"
                )
            }) || (fields.contains_key("id") && fields.contains_key("item"))
                || r.get("count")
                    .is_some_and(|v| v.as_u64().is_none_or(|v| !(1..=99).contains(&v)))
                || r.get("chance").is_some_and(|v| v.as_f64().is_none())
                || r.get("components").is_some_and(|v| !v.is_object())
            {
                return None;
            }
            let mut r = r.clone();
            if r.get("id").is_none() {
                r["id"] = r["item"].clone();
            }
            let stack = game.native_recipe_result(&r)?;
            let chance = r["chance"].as_f64().unwrap_or(1.0) as f32;
            (chance.is_finite() && (0.0..=1.0).contains(&chance)).then(|| rules::Roll {
                stack: basin::core_stack(game, &stack),
                chance,
            })
        })
        .collect();
    let outputs = outputs?;
    if inputs.len() + fluids.len() == 0
        || inputs.len() > 64
        || fluids.len() > 2
        || outputs.is_empty()
        || outputs.len() > 4
    {
        return None;
    }
    Some(Recipe {
        id: id.to_owned(),
        core: rules::Recipe {
            kind: rules::RecipeKind::Basin,
            item_ingredients: inputs.len(),
            fluid_amounts,
            outputs,
            fluid_outputs: Vec::new(),
            heat,
            duration: match json
                .get("processing_time")
                .or_else(|| json.get("processingTime"))
            {
                None => 0,
                Some(v) => i32::try_from(v.as_i64()?).ok()?,
            },
        },
        inputs,
        fluids,
    })
}
fn plain_ingredient(i: &serde_json::Value) -> Option<serde_json::Value> {
    if let Some(id) = i.as_str() {
        return id.contains(':').then(|| i.clone());
    }
    if let Some(children) = i.as_array() {
        if children.is_empty() {
            return None;
        }
        return children
            .iter()
            .map(plain_ingredient)
            .collect::<Option<Vec<_>>>()
            .map(serde_json::Value::Array);
    }
    if i.as_object()?.len() != 1 {
        return None;
    }
    super::create_port::ingredient(i)
}

thread_local! {
    static WORLDS: RefCell<SessionCache<BTreeMap<Operator, Recipes>>> = RefCell::new(SessionCache::default());
    // WorldTables replaces its contents in place, so a table address alone
    // cannot invalidate vanilla-only recipe/tag lookups on resource reload.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}
pub(super) fn reset(reload: bool) {
    if reload {
        GENERATION.with(|generation| generation.set(generation.get().wrapping_add(1)));
    } else {
        WORLDS.with(|cache| cache.borrow_mut().clear());
    }
}
fn with_recipes<R>(
    game: &Game<'_>,
    operator: Operator,
    action: impl FnOnce(&mut Recipes) -> R,
) -> R {
    WORLDS.with(|cache| {
        action(
            cache
                .borrow_mut()
                .dimension(std::ptr::from_ref(&game.world) as usize, game.dim.id())
                .entry(operator)
                .or_default(),
        )
    })
}
// MechanicalPress.canCompress/ItemHelper.matchAllIngredients compare the
// displayed item arrays in order. Predicate identity/components are not compared.
fn can_compress(ingredients: &[Ingredient]) -> bool {
    matches!(ingredients.len(), 4 | 9)
        && ingredients.iter().all(|i| i.order == ingredients[0].order)
}
fn recipes(game: &Game<'_>, operator: Operator) -> Vec<Arc<Recipe>> {
    with_recipes(game, operator, |cache| {
        let custom = game.native_recipes_of_type(operator.recipe_type());
        let table = game.recipes();
        let vanilla = std::ptr::from_ref(table) as usize;
        let generation = GENERATION.with(Cell::get);
        if cache.generation == generation
            && cache.vanilla == vanilla
            && cache.source.len() == custom.len()
            && custom
                .iter()
                .zip(&cache.source)
                .all(|(r, old)| Arc::ptr_eq(&r.json, old))
        {
            return cache.compiled.clone();
        }
        let mut compiled: Vec<_> = custom
            .iter()
            .filter_map(|r| compile(game, &r.id, &r.json))
            .map(Arc::new)
            .collect();
        for (id, recipe) in &table.crafting {
            if id.ends_with("_manual_only") {
                continue;
            }
            let (ingredients, result, shaped) = match recipe {
                Crafting::Shapeless {
                    ingredients,
                    result,
                } => (ingredients.clone(), result, false),
                Crafting::Shaped { cells, result, .. } if operator == Operator::Compacting => {
                    // An empty shaped cell is Ingredient.EMPTY in the original
                    // list, not an omitted ingredient. It must participate in
                    // both the 4/9 size test and displayed-array comparison.
                    let inputs = cells
                        .iter()
                        .map(|i| i.clone().unwrap_or_else(|| Ingredient::of(Vec::new())))
                        .collect();
                    (inputs, result, true)
                }
                _ => continue,
            };
            let compress = can_compress(&ingredients);
            let accepted = match operator {
                Operator::Mixing => !shaped && ingredients.len() > 1 && !compress,
                Operator::Compacting => compress,
            };
            if !accepted {
                continue;
            }
            let output = result.create();
            if output.count == 0 || output.item.as_ref() == "minecraft:air" {
                continue;
            }
            compiled.push(Arc::new(Recipe {
                id: horizon_client::crafting::recipe_id(id),
                inputs: ingredients.iter().cloned().map(Input::Item).collect(),
                fluids: Vec::new(),
                core: rules::Recipe {
                    kind: rules::RecipeKind::Crafting,
                    item_ingredients: ingredients.len(),
                    fluid_amounts: Vec::new(),
                    outputs: vec![rules::Roll {
                        stack: basin::core_stack(game, &output),
                        chance: 1.0,
                    }],
                    fluid_outputs: Vec::new(),
                    heat: rules::HeatRequirement::None,
                    duration: 0,
                },
            }));
        }
        cache.generation = generation;
        cache.vanilla = vanilla;
        cache.source = custom.iter().map(|r| r.json.clone()).collect();
        cache.compiled = compiled;
        cache.compiled.clone()
    })
}

struct Context<'a, 'b> {
    game: &'a mut Game<'b>,
    inputs: &'a [Input],
    fluids: &'a [String],
}
impl rules::Context<basin::Key, fluids::Key> for Context<'_, '_> {
    fn item_matches(&mut self, ingredient: usize, item: &rules::Item<basin::Key>) -> bool {
        match &self.inputs[ingredient] {
            Input::Item(input) => input.test(&item.key.0),
            Input::BlockTag(tag) => self
                .game
                .items()
                .get(&item.key.0.item)
                .and_then(|info| info.block)
                .and_then(|state| self.game.pack.assets.registry.state(state))
                .is_some_and(|state| {
                    self.game
                        .native_server_block_in_tag(state, tag)
                        .unwrap_or(false)
                }),
        }
    }
    fn fluid_matches(&mut self, ingredient: usize, fluid: &rules::Fluid<fluids::Key>) -> bool {
        // NeoForge single fluid ingredients match the fluid holder, ignoring
        // the stack component patch; consumed storage still keeps that patch.
        self.fluids[ingredient] == fluid.key.id
    }
    fn remainder(&mut self, unit: &rules::Item<basin::Key>) -> Option<rules::Item<basin::Key>> {
        let item = self
            .game
            .items()
            .get(&unit.key.0.item)?
            .crafting_remainder
            .clone()?;
        Some(basin::core_stack(self.game, &ItemStack::new(&item, 1)))
    }
    fn random(&mut self) -> f32 {
        self.game.world.random.next_float()
    }
}
pub(super) fn matching(
    game: &mut Game<'_>,
    pos: Pos,
    basin: &mut basin::Basin,
    recipe: &Recipe,
) -> bool {
    if recipe.core.kind == rules::RecipeKind::Basin {
        basin.heat = basin::heat(game, pos);
    }
    basin
        .match_recipe(
            &recipe.core,
            true,
            true,
            &mut Context {
                game,
                inputs: &recipe.inputs,
                fluids: &recipe.fluids,
            },
        )
        .success()
}
pub(super) fn choose(
    game: &mut Game<'_>,
    pos: Pos,
    basin: &mut basin::Basin,
    operator: Operator,
) -> Option<Arc<Recipe>> {
    if (basin.input.iter().chain(&basin.output).all(Option::is_none)
        && basin
            .input_fluids
            .iter()
            .chain(&basin.output_fluids)
            .all(Option::is_none))
        || !basin.can_continue_processing()
    {
        return None;
    }
    // Source tests EVERY candidate before sorting: even match simulations roll
    // probabilistic outputs. Do not short-circuit at the first successful one.
    let variants: std::collections::BTreeSet<_> = basin
        .input
        .iter()
        .chain(&basin.output)
        .flatten()
        .filter(|s| s.count > 0)
        .map(|s| s.key.0.item.to_string())
        .collect();
    let mut found: Vec<_> = recipes(game, operator)
        .into_iter()
        // RecipeTrie indexes simple ingredients by item variant, without
        // counts/components. Its universal key admits non-simple predicates.
        .filter(|r| {
            r.inputs.iter().all(|i| match i {
                Input::BlockTag(_) => true,
                Input::Item(i) => {
                    i.custom.is_some()
                        || i.children.is_some()
                        || i.order.iter().any(|item| variants.contains(item.as_str()))
                }
            })
        })
        .filter(|r| matching(game, pos, basin, r))
        .collect();
    found.sort_by_key(|r| std::cmp::Reverse(r.inputs.len()));
    found.into_iter().next()
}
pub(super) fn apply(game: &mut Game<'_>, pos: Pos, recipe: Option<&Recipe>) -> bool {
    let Some(recipe) = recipe else {
        return false;
    };
    let Some(mut state) = basin::read(game, pos) else {
        return false;
    };
    if recipe.core.kind == rules::RecipeKind::Basin {
        state.heat = basin::heat(game, pos);
    }
    let was_empty = state.can_continue_processing();
    let report = state.apply(
        &recipe.core,
        false,
        &mut Context {
            game,
            inputs: &recipe.inputs,
            fluids: &recipe.fluids,
        },
    );
    // Preserve known source partial-consumption/output-overbooking failures.
    if report.changed() {
        basin::write(game, pos, &state);
    }
    if !report.success() {
        return false;
    }
    was_empty && matching(game, pos, &mut state, recipe)
}