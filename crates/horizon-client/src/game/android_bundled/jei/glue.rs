// The edges between Horizon and the viewer (crate::game::jei): the game's
// item and recipe tables in, the panel, give and place out.
use crate::game::jei::{
    self, Context, Event, Ingredient, Item, Kind, Layout, Out, Recipe, Stack, View, Viewer,
};
use crate::game::native_hooks::{
    NativePanelEvent, NativeRecipeKind, NativeRecipeLayout, NativeRecipeStack, NativeRecipeView,
    NativeRecipes, NativeStack,
};
use std::cell::RefCell;

/// The viewer while a world is open.
struct State {
    viewer: Option<Viewer>,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State { viewer: None }) };
}

/// Builds the viewer once per world and shows the item list in the panel.
pub(super) fn sync(game: &mut crate::game::Game<'_>) {
    if !game.native_on(jei::ID) {
        return;
    }
    if game.native_player().is_none() {
        STATE.with(|s| s.borrow_mut().viewer = None);
        return;
    }
    if STATE.with(|s| s.borrow().viewer.is_some()) {
        return;
    }
    let Some(catalog) = game.native_recipes() else {
        return;
    };
    let lang = jei::lang(game.native_setting(jei::ID, "language").as_deref());
    let (viewer, out) = Viewer::new(items(&catalog), recipes(&catalog), lang);
    STATE.with(|s| s.borrow_mut().viewer = Some(viewer));
    if let Err(error) = apply(game, out) {
        game.native_chat(&format!("JEI: {error}"));
    }
}

/// The panel's events: search, clicks, R/U, tabs, pages and the action button.
pub(super) fn event(game: &mut crate::game::Game<'_>, event: &NativePanelEvent) {
    if !game.native_on(jei::ID) {
        return;
    }
    let Some(event) = convert_event(event) else {
        return;
    };
    let known = game.native_known_recipes();
    let recipe_key = game.native_mod_key(jei::ID, "recipe", "key.keyboard.r");
    let uses_key = game.native_mod_key(jei::ID, "uses", "key.keyboard.u");
    let give = jei::give(game.native_setting(jei::ID, "give").as_deref());
    let lang = jei::lang(game.native_setting(jei::ID, "language").as_deref());
    let ctx = Context {
        recipe_key: &recipe_key,
        uses_key: &uses_key,
        give,
        // `native_give` succeeds in creative only: try, and fall back below.
        creative: give,
        known: &known,
    };

    let mut fallback = false;
    for out in handle(&event, &ctx, lang) {
        if let Out::Give { item, count } = out {
            if game.native_give(&item, count).is_err() {
                fallback = true;
            }
            continue;
        }
        if let Err(error) = apply(game, out) {
            game.native_chat(&format!("JEI: {error}"));
        }
    }
    if fallback {
        // Survival: the same click opens the recipes, as JEI without cheat mode.
        let ctx = Context {
            give: false,
            creative: false,
            ..ctx
        };
        for out in handle(&event, &ctx, lang) {
            if let Err(error) = apply(game, out) {
                game.native_chat(&format!("JEI: {error}"));
            }
        }
    }
}

fn handle(event: &Event, ctx: &Context<'_>, lang: jei::Lang) -> Vec<Out> {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(viewer) = state.viewer.as_mut() else {
            return Vec::new();
        };
        viewer.set_lang(lang);
        viewer.handle(event.clone(), ctx)
    })
}

/// The viewer's size, for `/jei`; none outside a world.
pub(super) fn counts() -> Option<(usize, usize)> {
    STATE.with(|s| {
        s.borrow()
            .viewer
            .as_ref()
            .map(|viewer| (viewer.item_count(), viewer.recipe_count()))
    })
}

fn apply(game: &mut crate::game::Game<'_>, out: Out) -> Result<(), String> {
    match out {
        Out::Items(stacks) => {
            game.native_panel_items(jei::ID, panel_stacks(stacks));
            Ok(())
        }
        Out::Recipes {
            title,
            tabs,
            tab,
            views,
        } => {
            game.native_panel_recipes(
                jei::ID,
                &title,
                to_native(tabs),
                tab,
                views.into_iter().map(native_view).collect(),
            );
            Ok(())
        }
        Out::Give { item, count } => game.native_give(&item, count),
        Out::Place { book_id, all } => game.native_place_recipe(book_id, all),
        Out::Say(text) => {
            game.native_chat(&text);
            Ok(())
        }
    }
}

/// The game's stacks. An empty list keeps one invisible slot: the panel (and
/// its search box and events) goes away when the list is empty.
fn panel_stacks(stacks: Vec<Stack>) -> Vec<NativeStack> {
    if stacks.is_empty() {
        return vec![NativeStack {
            item: "minecraft:air".to_owned(),
            count: 0,
            components: String::new(),
        }];
    }
    to_native(stacks)
}

fn to_native(stacks: Vec<Stack>) -> Vec<NativeStack> {
    stacks
        .into_iter()
        .map(|s| NativeStack {
            item: s.item,
            count: s.count,
            components: s.components,
        })
        .collect()
}

fn native_view(view: View) -> NativeRecipeView {
    NativeRecipeView {
        layout: match view.layout {
            Layout::Grid => NativeRecipeLayout::Grid,
            Layout::Cooking => NativeRecipeLayout::Cooking,
            Layout::Single => NativeRecipeLayout::Single,
            Layout::Smithing => NativeRecipeLayout::Smithing,
            Layout::Brewing => NativeRecipeLayout::Brewing,
        },
        width: view.width,
        height: view.height,
        slots: view.slots.into_iter().map(to_native).collect(),
        results: to_native(view.results),
        note: view.note,
        action: view.action,
    }
}

fn convert_event(event: &NativePanelEvent) -> Option<Event> {
    Some(match event {
        NativePanelEvent::Search(text) => Event::Search(text.clone()),
        NativePanelEvent::Clicked { index, right } => Event::Clicked {
            index: *index as usize,
            right: *right,
        },
        NativePanelEvent::Key { key, stack } => Event::Key {
            key: key.clone(),
            stack: stack_of(stack),
        },
        NativePanelEvent::Tab(tab) => Event::Tab(*tab as usize),
        NativePanelEvent::Back => Event::Back,
        NativePanelEvent::SlotClicked { stack, right, .. } => Event::SlotClicked {
            stack: stack_of(stack),
            right: *right,
        },
        NativePanelEvent::Action(index) => Event::Action(*index as usize),
        _ => return None,
    })
}

fn stack_of(stack: &NativeStack) -> Stack {
    Stack {
        item: stack.item.clone(),
        count: stack.count,
        components: stack.components.clone(),
    }
}

fn items(catalog: &NativeRecipes) -> Vec<Item> {
    catalog
        .items
        .iter()
        .map(|item| Item {
            id: item.id.clone(),
            name: item.name.clone(),
            tags: item.tags.clone(),
        })
        .collect()
}

fn recipes(catalog: &NativeRecipes) -> Vec<Recipe> {
    catalog
        .recipes
        .iter()
        // A recipe with no ingredients says nothing in a list of recipes.
        .filter(|recipe| !recipe.slots.is_empty())
        .map(|recipe| Recipe {
            kind: kind(recipe.kind),
            width: recipe.width,
            height: recipe.height,
            slots: ingredient_slots(&recipe.slots),
            results: ingredient_list(&recipe.results),
            cooking_time: recipe.cooking_time,
            experience: recipe.experience,
            book_id: recipe.book_id,
        })
        .collect()
}

fn ingredient_slots(slots: &[Vec<NativeRecipeStack>]) -> Vec<Vec<Ingredient>> {
    slots.iter().map(|slot| ingredient_list(slot)).collect()
}

fn ingredient_list(stacks: &[NativeRecipeStack]) -> Vec<Ingredient> {
    stacks
        .iter()
        .map(|stack| Ingredient {
            item: stack.item,
            count: stack.count,
            components: stack.components.clone(),
        })
        .collect()
}

fn kind(kind: NativeRecipeKind) -> Kind {
    match kind {
        NativeRecipeKind::CraftingShaped => Kind::CraftingShaped,
        NativeRecipeKind::CraftingShapeless => Kind::CraftingShapeless,
        NativeRecipeKind::Smelting => Kind::Smelting,
        NativeRecipeKind::Blasting => Kind::Blasting,
        NativeRecipeKind::Smoking => Kind::Smoking,
        NativeRecipeKind::Campfire => Kind::Campfire,
        NativeRecipeKind::Stonecutting => Kind::Stonecutting,
        NativeRecipeKind::Smithing => Kind::Smithing,
        NativeRecipeKind::Brewing => Kind::Brewing,
    }
}
