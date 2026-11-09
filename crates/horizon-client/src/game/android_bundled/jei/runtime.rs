// JEI for Horizon: an item and recipe viewer built on Horizon's native hook
// points. This file is both the mod's Cargo library (its tests run with
// `cargo test`) and the module the bundle adds to the game as crate::game::jei.
//
// Phase 0: the bundle's skeleton. It registers its row in Options > Mods, a
// settings page with one toggle, and the /jei command, to prove the wiring
// before the viewer itself (tasks 1.x and 2.x in tasks/todo.md).

/// The mod's id: its row, its settings folder and its keys.
pub const ID: &str = "jei";
pub const NAME: &str = "JEI";
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str = "An item and recipe viewer: every item beside container screens, with search, recipes and uses.";

/// Its keys: name, label and default (the Java mod's R and U).
pub const KEYS: [[&str; 3]; 2] = [
    ["recipe", "Show recipes", "key.keyboard.r"],
    ["uses", "Show uses", "key.keyboard.u"],
];

/// `JEI 0.1.0`, as Options > Mods lists it.
pub fn title() -> String {
    format!("{NAME} {VERSION}")
}

/// The line `/jei` says in chat.
pub fn about() -> String {
    format!("{NAME} {VERSION} (alpha) on hook points version 2")
}

/// The line `/jei` says in a world: what the viewer holds.
pub fn about_counts(items: usize, recipes: usize) -> String {
    format!("{}: {items} items, {recipes} recipes", about())
}

/// The UI's language: the game's language is not exposed to mods, so the
/// player picks one here (default English).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

pub fn lang(saved: Option<&str>) -> Lang {
    if saved == Some("es") {
        Lang::Es
    } else {
        Lang::En
    }
}

impl Lang {
    pub fn next(self) -> Lang {
        match self {
            Lang::En => Lang::Es,
            Lang::Es => Lang::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
        }
    }

    pub fn language_button(self) -> String {
        format!(
            "Language: {}",
            match self {
                Lang::En => "English",
                Lang::Es => "Español",
            }
        )
    }

    pub fn give_button(self, on: bool) -> String {
        let state = if on { "ON" } else { "OFF" };
        match self {
            Lang::En => format!("Click gives items: {state}"),
            Lang::Es => format!("Clic da objetos: {state}"),
        }
    }

    fn recipes(self) -> &'static str {
        match self {
            Lang::En => "Recipes",
            Lang::Es => "Recetas",
        }
    }

    fn uses(self) -> &'static str {
        match self {
            Lang::En => "Uses",
            Lang::Es => "Usos",
        }
    }

    fn shapeless(self) -> &'static str {
        match self {
            Lang::En => "Shapeless",
            Lang::Es => "Sin forma",
        }
    }

    fn no_recipes(self) -> &'static str {
        match self {
            Lang::En => "No recipes for",
            Lang::Es => "Sin recetas para",
        }
    }

    fn no_uses(self) -> &'static str {
        match self {
            Lang::En => "No uses for",
            Lang::Es => "Sin usos para",
        }
    }
}

/// Whether "Click gives items" is on: `true` only when the player turned it on.
pub fn give(saved: Option<&str>) -> bool {
    saved == Some("true")
}

/// The settings page's button for "Click gives items", in English.
pub fn give_button(on: bool) -> String {
    Lang::En.give_button(on)
}

/// `/jei` and `/jei give`, without the slash. `None` for another command.
pub fn command(args: &str) -> Option<Command> {
    match args.trim() {
        "" | "about" => Some(Command::About),
        "give" => Some(Command::ToggleGive),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    About,
    ToggleGive,
}

/// One word of a search, as JEI's `SearchTokenizer` builds it (MIT, © mezz:
/// `Gui/src/main/java/mezz/jei/gui/search/SearchTokenizer.java`). `start` and
/// `end` are character offsets, for highlighting; `operator` is a `|` between
/// alternatives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    pub exclusion: bool,
    pub operator: bool,
    pub start: usize,
    pub end: usize,
}

impl Token {
    fn operator(at: usize) -> Self {
        Self {
            text: "|".to_owned(),
            exclusion: false,
            operator: true,
            start: at,
            end: at + 1,
        }
    }
}

/// Splits a search into tokens: words, quoted phrases, `-exclusions` and `|`
/// alternatives. Escapes (`\"`) keep a quote in a word or phrase. Unicode
/// offsets count characters, not UTF-16 units.
pub fn tokenize(filter: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    if filter.is_empty() {
        return tokens;
    }
    let mut current = String::new();
    let mut inside_quotes = false;
    let mut exclusion = false;
    let mut escaped = false;
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;

    for (at, c) in filter.char_indices() {
        if escaped {
            current.push(c);
            end = Some(at + c.len_utf8());
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            start.get_or_insert(at);
            end = Some(at + 1);
            continue;
        }
        if c == '"' {
            if inside_quotes {
                add_token(&mut tokens, &current, exclusion, start, Some(at + 1));
                current.clear();
                inside_quotes = false;
                exclusion = false;
                start = None;
                end = None;
            } else if !current.is_empty() && !is_prefix(&current) {
                add_token(&mut tokens, &current, exclusion, start, end);
                current.clear();
                exclusion = false;
                start = None;
                end = None;
            } else {
                inside_quotes = true;
                start.get_or_insert(at);
                end = Some(at + 1);
            }
            continue;
        }
        if !inside_quotes && c.is_whitespace() {
            if !current.is_empty() {
                add_token(&mut tokens, &current, exclusion, start, end);
                current.clear();
            }
            exclusion = false;
            start = None;
            end = None;
            continue;
        }
        if !inside_quotes && c == '|' {
            if !current.is_empty() {
                add_token(&mut tokens, &current, exclusion, start, end);
                current.clear();
            }
            tokens.push(Token::operator(at));
            exclusion = false;
            start = None;
            end = None;
            continue;
        }
        if !inside_quotes && current.is_empty() && c == '-' {
            exclusion = true;
            start = Some(at);
            end = Some(at + 1);
            continue;
        }
        start.get_or_insert(at);
        end = Some(at + c.len_utf8());
        current.push(c);
    }

    if !current.is_empty() || inside_quotes {
        add_token(&mut tokens, &current, exclusion, start, end);
    }
    tokens
}

fn add_token(
    tokens: &mut Vec<Token>,
    content: &str,
    exclusion: bool,
    start: Option<usize>,
    end: Option<usize>,
) {
    let text = content.trim();
    if !text.is_empty() {
        tokens.push(Token {
            text: text.to_owned(),
            exclusion,
            operator: false,
            start: start.unwrap_or_default(),
            end: end.unwrap_or_default(),
        });
    }
}

/// A single character that is not a word, `-` or `|`: the `@`, `#`, `$`, `%`
/// prefixes, which mark a quoted phrase as prefixed (`@"two words"`).
fn is_prefix(text: &str) -> bool {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return false;
    };
    !c.is_alphanumeric() && c != '-' && c != '|'
}

/// The tokens grouped by `|`: each group is one alternative of the search; a
/// group is a conjunction of its tokens.
pub fn split_by_operator(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut groups: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    for token in tokens {
        if token.operator {
            groups.push(std::mem::take(&mut current));
        } else {
            current.push(token.clone());
        }
    }
    groups.push(current);
    groups
}

/// One item as the search sees it.
pub struct SearchItem<'a> {
    pub name: &'a str,
    pub id: &'a str,
    /// Tags without the `#` (`minecraft:logs`).
    pub tags: &'a [String],
}

/// Whether `item` passes the search: any `|` alternative matches, and every
/// token of it matches. Prefixes: `@` namespace, `#` tag; `$` (tooltip) and
/// `%` (creative tab) have no data yet and match nothing.
pub fn keeps(item: &SearchItem<'_>, search: &str) -> bool {
    let groups = split_by_operator(&tokenize(search));
    groups
        .iter()
        .any(|group| group.iter().all(|token| token_matches(item, token)))
}

fn token_matches(item: &SearchItem<'_>, token: &Token) -> bool {
    let text = token.text.to_lowercase();
    let hit = if let Some(namespace) = text.strip_prefix('@') {
        item.id
            .split(':')
            .next()
            .is_some_and(|ns| ns.starts_with(namespace))
    } else if let Some(tag) = text.strip_prefix('#') {
        item.tags.iter().any(|t| t.to_lowercase().contains(tag))
    } else if text.starts_with('$') || text.starts_with('%') {
        false
    } else {
        item.name.to_lowercase().contains(&text) || item.id.to_lowercase().contains(&text)
    };
    hit != token.exclusion
}

/// Items the panel takes, and the most recipes a page shows.
pub const ITEMS_MAX: usize = 8192;
pub const RECIPES_MAX: usize = 256;
/// Stacks a slot cycles through.
pub const STACKS_MAX: usize = 64;

/// One item of the game, as the viewer lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
}

impl Item {
    fn search(&self) -> SearchItem<'_> {
        SearchItem {
            name: &self.name,
            id: &self.id,
            tags: &self.tags,
        }
    }
}

/// A stack the panel shows: an item id, a count and components in vanilla's
/// item syntax, or "".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: u32,
    pub components: String,
}

impl Stack {
    pub fn item(id: &str) -> Self {
        Self {
            item: id.to_owned(),
            count: 1,
            components: String::new(),
        }
    }
}

/// One alternative of a recipe slot: an index into the viewer's items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ingredient {
    pub item: u32,
    pub count: u32,
    pub components: String,
}

/// The vanilla recipe kinds the game gives a viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    CraftingShaped,
    CraftingShapeless,
    Smelting,
    Blasting,
    Smoking,
    Campfire,
    Stonecutting,
    Smithing,
    Brewing,
}

/// The station a kind is made at, in the order of the tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Station {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
    Campfire,
    Stonecutter,
    Smithing,
    Brewing,
}

impl Station {
    pub const ALL: [Station; 8] = [
        Station::Crafting,
        Station::Furnace,
        Station::BlastFurnace,
        Station::Smoker,
        Station::Campfire,
        Station::Stonecutter,
        Station::Smithing,
        Station::Brewing,
    ];

    /// The item whose icon stands for the station in the tabs.
    pub fn icon(self) -> &'static str {
        match self {
            Station::Crafting => "minecraft:crafting_table",
            Station::Furnace => "minecraft:furnace",
            Station::BlastFurnace => "minecraft:blast_furnace",
            Station::Smoker => "minecraft:smoker",
            Station::Campfire => "minecraft:campfire",
            Station::Stonecutter => "minecraft:stonecutter",
            Station::Smithing => "minecraft:smithing_table",
            Station::Brewing => "minecraft:brewing_stand",
        }
    }

    fn order(self) -> usize {
        Station::ALL
            .iter()
            .position(|s| *s == self)
            .unwrap_or(Station::ALL.len())
    }
}

impl Kind {
    pub fn station(self) -> Station {
        match self {
            Kind::CraftingShaped | Kind::CraftingShapeless => Station::Crafting,
            Kind::Smelting => Station::Furnace,
            Kind::Blasting => Station::BlastFurnace,
            Kind::Smoking => Station::Smoker,
            Kind::Campfire => Station::Campfire,
            Kind::Stonecutting => Station::Stonecutter,
            Kind::Smithing => Station::Smithing,
            Kind::Brewing => Station::Brewing,
        }
    }
}

/// A recipe as the viewer knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub kind: Kind,
    /// The grid's size for a shaped recipe; 0 for the other kinds.
    pub width: u32,
    pub height: u32,
    /// Each slot's alternatives; the input(s) for the non-grid kinds.
    pub slots: Vec<Vec<Ingredient>>,
    pub results: Vec<Ingredient>,
    /// Game ticks and experience, for cooking; 0 otherwise.
    pub cooking_time: u32,
    pub experience: f32,
    /// The recipe book's id, or -1.
    pub book_id: i32,
}

/// How the panel lays a recipe out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Grid,
    Cooking,
    Single,
    Smithing,
    Brewing,
}

/// A recipe as the panel draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub layout: Layout,
    pub width: u32,
    pub height: u32,
    pub slots: Vec<Vec<Stack>>,
    pub results: Vec<Stack>,
    pub note: String,
    /// The action button: none, or whether it is enabled.
    pub action: Option<bool>,
}

/// What the player did, in the viewer's words.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Search(String),
    Clicked { index: usize, right: bool },
    Key { key: String, stack: Stack },
    Tab(usize),
    Back,
    SlotClicked { stack: Stack, right: bool },
    Action(usize),
}

/// What the game is to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Show these items in the panel; empty clears it.
    Items(Vec<Stack>),
    /// Show this recipe page; no views goes back to the items.
    Recipes {
        title: String,
        tabs: Vec<Stack>,
        tab: u32,
        views: Vec<View>,
    },
    /// Creative: give the player this item.
    Give { item: String, count: u32 },
    /// Place this recipe-book id in the open grid.
    Place { book_id: i32, all: bool },
    /// A line only this player sees.
    Say(String),
}

/// The player's settings and the book's state, for one event.
pub struct Context<'a> {
    pub recipe_key: &'a str,
    pub uses_key: &'a str,
    /// "Click gives items" is on.
    pub give: bool,
    /// The game would give items (creative).
    pub creative: bool,
    /// The recipe book's ids now.
    pub known: &'a [i32],
}

/// One page of the history: an item's recipes or uses, on one station's tab.
#[derive(Clone, Debug, PartialEq)]
struct Page {
    item: Stack,
    uses: bool,
    tab: usize,
}

/// The viewer: the list, the search and the history of recipe pages.
pub struct Viewer {
    items: Vec<Item>,
    recipes: Vec<Recipe>,
    lang: Lang,
    /// The panel's items now: indexes into `items`, after the search.
    listed: Vec<usize>,
    search: String,
    /// The page shown, then the ones Back returns to.
    history: Vec<Page>,
    /// The book id behind each recipe the page shows (-1: none).
    shown: Vec<i32>,
}

impl Viewer {
    /// A viewer with the world's tables; the output lists every item.
    pub fn new(items: Vec<Item>, recipes: Vec<Recipe>, lang: Lang) -> (Self, Out) {
        let mut me = Self {
            items,
            recipes,
            lang,
            listed: Vec::new(),
            search: String::new(),
            history: Vec::new(),
            shown: Vec::new(),
        };
        let out = me.list();
        (me, out)
    }

    /// The player changed the language: rewrite the strings of the next page.
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn recipe_count(&self) -> usize {
        self.recipes.len()
    }

    fn id(&self, item: u32) -> &str {
        self.items
            .get(item as usize)
            .map_or("minecraft:air", |i| &i.id)
    }

    fn name(&self, item: &str) -> String {
        self.items
            .iter()
            .find(|i| i.id == item)
            .map_or_else(|| item.to_owned(), |i| i.name.clone())
    }

    fn convert(&self, ingredient: &Ingredient) -> Stack {
        Stack {
            item: self.id(ingredient.item).to_owned(),
            count: ingredient.count,
            components: ingredient.components.clone(),
        }
    }

    /// The same item, and the same components when `want` names some.
    fn matches(&self, ingredient: &Ingredient, want: &Stack) -> bool {
        self.id(ingredient.item) == want.item
            && (want.components.is_empty() || ingredient.components == want.components)
    }

    /// The items the search keeps: JEI's words, phrases, exclusions and
    /// prefixes.
    fn list(&mut self) -> Out {
        self.listed = (self.items.iter().enumerate())
            .filter(|(_, item)| item.id != "minecraft:air")
            .filter(|(_, item)| keeps(&item.search(), &self.search))
            .map(|(n, _)| n)
            .take(ITEMS_MAX)
            .collect();
        Out::Items(
            (self.listed.iter())
                .map(|&n| Stack::item(&self.items[n].id))
                .collect(),
        )
    }

    /// The recipes the current page lists, across every station.
    fn found(&self, page: &Page) -> Vec<&Recipe> {
        (self.recipes.iter())
            .filter(|r| {
                if page.uses {
                    r.slots
                        .iter()
                        .flatten()
                        .any(|s| self.matches(s, &page.item))
                } else {
                    r.results.iter().any(|s| self.matches(s, &page.item))
                }
            })
            .collect()
    }

    /// Opens a page, pushing it on the history; says why when it is empty.
    fn open(&mut self, item: &Stack, uses: bool, known: &[i32]) -> Vec<Out> {
        let item = Stack {
            count: 1,
            ..item.clone()
        };
        let page = Page { item, uses, tab: 0 };
        if self.found(&page).is_empty() {
            let name = self.name(&page.item.item).to_owned();
            return vec![Out::Say(format!(
                "{}: {name}",
                if uses {
                    self.lang.no_uses()
                } else {
                    self.lang.no_recipes()
                }
            ))];
        }
        self.history.push(page);
        vec![self.show(known)]
    }

    /// The page on top of the history, or the items when there is none.
    fn show(&mut self, known: &[i32]) -> Out {
        let Some(mut page) = self.history.last().cloned() else {
            self.shown.clear();
            return Out::Recipes {
                title: String::new(),
                tabs: Vec::new(),
                tab: 0,
                views: Vec::new(),
            };
        };
        let found = self.found(&page);
        let mut stations: Vec<Station> = found
            .iter()
            .map(|r| r.kind.station())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        stations.sort_by_key(|s| s.order());
        page.tab = page.tab.min(stations.len().saturating_sub(1));
        let selected = stations.get(page.tab).copied();
        let on_station: Vec<&Recipe> = found
            .iter()
            .copied()
            .filter(|r| Some(r.kind.station()) == selected)
            .collect();
        let total = on_station.len();
        let views: Vec<View> = on_station
            .iter()
            .take(RECIPES_MAX)
            .map(|&r| self.view(r, known))
            .collect();
        self.shown = on_station
            .iter()
            .take(RECIPES_MAX)
            .map(|r| r.book_id)
            .collect();
        let mut title = format!(
            "{}: {}",
            if page.uses {
                self.lang.uses()
            } else {
                self.lang.recipes()
            },
            self.name(&page.item.item)
        );
        if total > views.len() {
            title.push_str(&format!(" ({} of {total})", views.len()));
        }
        let tabs = (stations.iter()).map(|s| Stack::item(s.icon())).collect();
        if let Some(top) = self.history.last_mut() {
            top.tab = page.tab;
        }
        Out::Recipes {
            title,
            tabs,
            tab: page.tab as u32,
            views,
        }
    }

    /// A recipe as the panel draws it.
    fn view(&self, recipe: &Recipe, known: &[i32]) -> View {
        let slots = (recipe.slots.iter())
            .map(|slot| {
                slot.iter()
                    .take(STACKS_MAX)
                    .map(|s| self.convert(s))
                    .collect()
            })
            .collect();
        let results = (recipe.results.iter())
            .take(STACKS_MAX)
            .map(|s| self.convert(s))
            .collect();
        let (layout, width, height, note) = match recipe.kind {
            Kind::CraftingShaped => (Layout::Grid, recipe.width, recipe.height, String::new()),
            Kind::CraftingShapeless => {
                let n = recipe.slots.len() as u32;
                let width = match n {
                    0 | 1 => 1,
                    2..=4 => 2,
                    _ => 3,
                };
                (
                    Layout::Grid,
                    width,
                    n.div_ceil(width).max(1),
                    self.lang.shapeless().into(),
                )
            }
            Kind::Smelting | Kind::Blasting | Kind::Smoking | Kind::Campfire => {
                let seconds = recipe.cooking_time as f32 / 20.0;
                (
                    Layout::Cooking,
                    1,
                    1,
                    format!(
                        "{seconds} s, {} XP",
                        (recipe.experience * 100.0).round() / 100.0
                    ),
                )
            }
            Kind::Stonecutting => (Layout::Single, 1, 1, String::new()),
            Kind::Smithing => (Layout::Smithing, 3, 1, String::new()),
            Kind::Brewing => (Layout::Brewing, 2, 1, String::new()),
        };
        View {
            layout,
            width,
            height,
            slots,
            results,
            note,
            action: (recipe.book_id >= 0).then(|| known.contains(&recipe.book_id)),
        }
    }

    /// The player's event.
    pub fn handle(&mut self, event: Event, ctx: &Context<'_>) -> Vec<Out> {
        match event {
            Event::Search(text) => {
                self.search = text;
                vec![self.list()]
            }
            Event::Clicked { index, right } => {
                let Some(&n) = self.listed.get(index) else {
                    return Vec::new();
                };
                let item = self.items[n].id.clone();
                if ctx.give && ctx.creative {
                    return vec![Out::Give {
                        item,
                        count: if right { 99 } else { 1 },
                    }];
                }
                self.open(&Stack::item(&item), right, ctx.known)
            }
            Event::Key { key, stack } => {
                if key == ctx.recipe_key {
                    self.open(&stack, false, ctx.known)
                } else if key == ctx.uses_key {
                    self.open(&stack, true, ctx.known)
                } else {
                    Vec::new()
                }
            }
            Event::Tab(tab) => {
                if let Some(page) = self.history.last_mut() {
                    page.tab = tab;
                }
                vec![self.show(ctx.known)]
            }
            Event::Back => {
                self.history.pop();
                vec![self.show(ctx.known)]
            }
            Event::SlotClicked { stack, right } => self.open(&stack, right, ctx.known),
            Event::Action(i) => match self.shown.get(i) {
                Some(&book) if book >= 0 => vec![Out::Place {
                    book_id: book,
                    all: false,
                }],
                _ => Vec::new(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_has_name_and_version() {
        assert_eq!(super::title(), "JEI 0.1.0");
    }

    #[test]
    fn about_names_the_hook_points() {
        assert!(super::about().contains("hook points version 2"));
    }

    #[test]
    fn about_counts_lists_the_tables() {
        let text = super::about_counts(1658, 1234);
        assert!(text.contains("1658 items, 1234 recipes"), "{text}");
    }

    #[test]
    fn give_is_off_until_turned_on() {
        assert!(!super::give(None));
        assert!(!super::give(Some("false")));
        assert!(super::give(Some("true")));
    }

    #[test]
    fn commands() {
        assert_eq!(super::command(""), Some(super::Command::About));
        assert_eq!(super::command("  about "), Some(super::Command::About));
        assert_eq!(super::command("give"), Some(super::Command::ToggleGive));
        assert_eq!(super::command("config"), None);
    }

    // SearchTokenizerTest (JEI, MIT): the same cases, with character offsets.
    fn token(text: &str, exclusion: bool, start: usize, end: usize) -> super::Token {
        super::Token {
            text: text.to_owned(),
            exclusion,
            operator: false,
            start,
            end,
        }
    }

    #[test]
    fn search_simple_words() {
        assert_eq!(
            super::tokenize("iron sword"),
            vec![token("iron", false, 0, 4), token("sword", false, 5, 10),]
        );
    }

    #[test]
    fn search_quoted_phrase() {
        assert_eq!(
            super::tokenize("\"iron sword\""),
            vec![token("iron sword", false, 0, 12)]
        );
        assert_eq!(
            super::tokenize("\"iron sword"),
            vec![token("iron sword", false, 0, 11)]
        );
    }

    #[test]
    fn search_exclusion() {
        assert_eq!(
            super::tokenize("-diamond"),
            vec![token("diamond", true, 0, 8)]
        );
        assert_eq!(
            super::tokenize("-\"iron sword\""),
            vec![token("iron sword", true, 0, 13)]
        );
    }

    #[test]
    fn search_dash_inside_quotes_is_text() {
        assert_eq!(
            super::tokenize("\"-iron sword\""),
            vec![token("-iron sword", false, 0, 13)]
        );
        assert_eq!(
            super::tokenize("-\"-iron sword\""),
            vec![token("-iron sword", true, 0, 14)]
        );
    }

    #[test]
    fn search_mixed() {
        assert_eq!(
            super::tokenize("sword \"iron pick\" -wood"),
            vec![
                token("sword", false, 0, 5),
                token("iron pick", false, 6, 17),
                token("wood", true, 18, 23),
            ]
        );
    }

    #[test]
    fn search_empty_and_whitespace() {
        assert!(super::tokenize("").is_empty());
        assert!(super::tokenize("   ").is_empty());
        assert!(super::tokenize("-   ").is_empty());
        assert!(super::tokenize(" \"  ").is_empty());
        assert!(super::tokenize("\"\"").is_empty());
    }

    #[test]
    fn search_escaped_quotes() {
        assert_eq!(
            super::tokenize("iron\\\"sword \\\"quoted\\\""),
            vec![
                token("iron\"sword", false, 0, 11),
                token("\"quoted\"", false, 12, 22),
            ]
        );
        assert_eq!(
            super::tokenize("\"the \\\"best\\\" sword\""),
            vec![token("the \"best\" sword", false, 0, 20)]
        );
    }

    #[test]
    fn search_prefixed_quoted_phrases() {
        assert_eq!(
            super::tokenize("%\"redstone blocks\""),
            vec![token("%redstone blocks", false, 0, 18)]
        );
        assert_eq!(
            super::tokenize("-%\"redstone blocks\""),
            vec![token("%redstone blocks", true, 0, 19)]
        );
        assert_eq!(
            super::tokenize("%\"redstone blocks"),
            vec![token("%redstone blocks", false, 0, 17)]
        );
    }

    #[test]
    fn search_trims_inside_quotes() {
        assert_eq!(
            super::tokenize("\"  iron sword  \" -\"  gold ingot  \""),
            vec![
                token("iron sword", false, 0, 16),
                token("gold ingot", true, 17, 34),
            ]
        );
    }

    #[test]
    fn search_whitespace_separates() {
        assert_eq!(
            super::tokenize("iron\t\"gold ingot\"\n-wood\r\n\"red\tstone\""),
            vec![
                token("iron", false, 0, 4),
                token("gold ingot", false, 5, 17),
                token("wood", true, 18, 23),
                token("red\tstone", false, 25, 36),
            ]
        );
    }

    #[test]
    fn search_bare_dash_does_not_exclude() {
        assert_eq!(
            super::tokenize("- diamond - \"iron sword\""),
            vec![
                token("diamond", false, 2, 9),
                token("iron sword", false, 12, 24),
            ]
        );
        assert_eq!(
            super::tokenize("-\"\" diamond -\"  \" gold"),
            vec![token("diamond", false, 4, 11), token("gold", false, 18, 22),]
        );
    }

    #[test]
    fn search_dash_in_a_word_is_not_exclusion() {
        assert_eq!(
            super::tokenize("iron-sword @mod-name %-redstone"),
            vec![
                token("iron-sword", false, 0, 10),
                token("@mod-name", false, 11, 20),
                token("%-redstone", false, 21, 31),
            ]
        );
    }

    #[test]
    fn search_or_operator() {
        assert_eq!(
            super::tokenize("iron|sword"),
            vec![
                token("iron", false, 0, 4),
                super::Token::operator(4),
                token("sword", false, 5, 10),
            ]
        );
        assert_eq!(
            super::tokenize("iron | sword"),
            vec![
                token("iron", false, 0, 4),
                super::Token::operator(5),
                token("sword", false, 7, 12),
            ]
        );
        assert_eq!(
            super::tokenize("\"iron|sword\""),
            vec![token("iron|sword", false, 0, 12)]
        );
    }

    #[test]
    fn search_groups_by_or() {
        let groups = super::split_by_operator(&super::tokenize("iron|gold sword"));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 1);
        assert_eq!(groups[0][0].text, "iron");
        assert_eq!(groups[1].len(), 2);
        assert_eq!(groups[1][0].text, "gold");
        assert_eq!(groups[1][1].text, "sword");
    }

    fn stone<'a>(tags: &'a [String]) -> super::SearchItem<'a> {
        super::SearchItem {
            name: "Stone",
            id: "minecraft:stone",
            tags,
        }
    }

    #[test]
    fn search_keeps_by_name_and_id() {
        let tags = ["minecraft:logs".to_owned()];
        let item = stone(&tags);
        assert!(super::keeps(&item, ""));
        assert!(super::keeps(&item, "stone"));
        assert!(super::keeps(&item, "STONE"));
        assert!(super::keeps(&item, "minecraft:sto"));
        assert!(!super::keeps(&item, "diamond"));
        assert!(!super::keeps(&item, "-stone"));
        assert!(super::keeps(&item, "-diamond"));
    }

    #[test]
    fn search_keeps_by_prefix() {
        let tags = ["minecraft:logs".to_owned()];
        let item = stone(&tags);
        assert!(super::keeps(&item, "@minecraft"));
        assert!(!super::keeps(&item, "@other"));
        assert!(super::keeps(&item, "#logs"));
        assert!(!super::keeps(&item, "#stone"));
        // No tooltip or creative-tab data yet: those prefixes match nothing.
        assert!(!super::keeps(&item, "$anything"));
        assert!(!super::keeps(&item, "%anything"));
    }

    #[test]
    fn search_keeps_or_and_phrases() {
        let tags: [String; 0] = [];
        let item = stone(&tags);
        assert!(super::keeps(&item, "iron|stone"));
        assert!(!super::keeps(&item, "iron|gold"));
        assert!(super::keeps(&item, "\"one stone\"|stone"));
        assert!(!super::keeps(&item, "\"one stone\""));
    }

    // The viewer: the list, station tabs, history, give and transfer.
    fn item(id: &str, name: &str) -> super::Item {
        super::Item {
            id: id.to_owned(),
            name: name.to_owned(),
            tags: Vec::new(),
        }
    }

    fn ingredient(item: u32) -> super::Ingredient {
        super::Ingredient {
            item,
            count: 1,
            components: String::new(),
        }
    }

    fn stack(id: &str) -> super::Stack {
        super::Stack::item(id)
    }

    /// Stone, Diamond, Diamond Block, Diamond Ore, Crafting Table, Furnace.
    fn tables() -> (Vec<super::Item>, Vec<super::Recipe>) {
        let items = vec![
            item("minecraft:stone", "Stone"),
            item("minecraft:diamond", "Diamond"),
            item("minecraft:diamond_block", "Block of Diamond"),
            item("minecraft:diamond_ore", "Diamond Ore"),
            item("minecraft:crafting_table", "Crafting Table"),
            item("minecraft:furnace", "Furnace"),
        ];
        let recipes = vec![
            // Block of Diamond -> Diamond (shapeless), book 7.
            super::Recipe {
                kind: super::Kind::CraftingShapeless,
                width: 0,
                height: 0,
                slots: vec![vec![ingredient(2)]],
                results: vec![ingredient(1)],
                cooking_time: 0,
                experience: 0.0,
                book_id: 7,
            },
            // Diamond Ore -> Diamond (smelting), book 8.
            super::Recipe {
                kind: super::Kind::Smelting,
                width: 0,
                height: 0,
                slots: vec![vec![ingredient(3)]],
                results: vec![ingredient(1)],
                cooking_time: 200,
                experience: 1.0,
                book_id: 8,
            },
            // Diamond -> Block of Diamond (shaped 1x1), book 9.
            super::Recipe {
                kind: super::Kind::CraftingShaped,
                width: 1,
                height: 1,
                slots: vec![vec![ingredient(1)]],
                results: vec![ingredient(2)],
                cooking_time: 0,
                experience: 0.0,
                book_id: 9,
            },
            // A smithing recipe the book doesn't show.
            super::Recipe {
                kind: super::Kind::Smithing,
                width: 0,
                height: 0,
                slots: vec![
                    vec![ingredient(0)],
                    vec![ingredient(1)],
                    vec![ingredient(2)],
                ],
                results: vec![ingredient(1)],
                cooking_time: 0,
                experience: 0.0,
                book_id: -1,
            },
        ];
        (items, recipes)
    }

    fn context(known: &[i32]) -> super::Context<'_> {
        super::Context {
            recipe_key: "key.keyboard.r",
            uses_key: "key.keyboard.u",
            give: false,
            creative: false,
            known,
        }
    }

    #[test]
    fn viewer_lists_items_and_filters() {
        let (items, recipes) = tables();
        let (mut viewer, out) = super::Viewer::new(items, recipes, super::Lang::En);
        assert!(matches!(&out, super::Out::Items(list) if list.len() == 6));
        let out = viewer.handle(super::Event::Search("diamond".into()), &context(&[]));
        assert!(matches!(&out[0], super::Out::Items(list) if list.len() == 3));
        let out = viewer.handle(super::Event::Search(String::new()), &context(&[]));
        assert!(matches!(&out[0], super::Out::Items(list) if list.len() == 6));
    }

    #[test]
    fn viewer_opens_recipes_on_station_tabs() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        let out = viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[7]),
        );
        let super::Out::Recipes {
            title,
            tabs,
            tab,
            views,
        } = &out[0]
        else {
            panic!("not a page: {out:?}");
        };
        assert_eq!(title, "Recipes: Diamond");
        assert_eq!(*tab, 0);
        assert_eq!(tabs[0].item, "minecraft:crafting_table");
        assert_eq!(tabs[1].item, "minecraft:furnace");
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].layout, super::Layout::Grid);
        assert_eq!(views[0].note, "Shapeless");
        // The book holds recipe 7: the "+" is enabled.
        assert_eq!(views[0].action, Some(true));

        let out = viewer.handle(super::Event::Tab(1), &context(&[7]));
        let super::Out::Recipes { tab, views, .. } = &out[0] else {
            panic!("not a page: {out:?}");
        };
        assert_eq!(*tab, 1);
        assert_eq!(views[0].layout, super::Layout::Cooking);
        assert_eq!(views[0].note, "10 s, 1 XP");
    }

    #[test]
    fn viewer_uses_find_every_input() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        let out = viewer.handle(
            super::Event::Key {
                key: "key.keyboard.u".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[]),
        );
        let super::Out::Recipes { title, tabs, .. } = &out[0] else {
            panic!("not a page: {out:?}");
        };
        assert_eq!(title, "Uses: Diamond");
        // Crafting (diamond -> block) and smithing (diamond is its base).
        assert_eq!(tabs.len(), 2, "one tab per station: {tabs:?}");
    }

    #[test]
    fn viewer_back_walks_the_history() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[]),
        );
        viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:diamond_block"),
            },
            &context(&[]),
        );
        let out = viewer.handle(super::Event::Back, &context(&[]));
        assert!(
            matches!(&out[0], super::Out::Recipes { title, .. } if title == "Recipes: Diamond")
        );
        let out = viewer.handle(super::Event::Back, &context(&[]));
        assert!(matches!(&out[0], super::Out::Recipes { views, .. } if views.is_empty()));
    }

    #[test]
    fn viewer_gives_only_in_creative() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        let mut ctx = context(&[]);
        ctx.give = true;
        // Not creative: the click opens the item's recipes instead.
        let out = viewer.handle(
            super::Event::Clicked {
                index: 1,
                right: false,
            },
            &ctx,
        );
        assert!(matches!(&out[0], super::Out::Recipes { .. }));
        ctx.creative = true;
        let out = viewer.handle(
            super::Event::Clicked {
                index: 1,
                right: false,
            },
            &ctx,
        );
        assert!(matches!(&out[0], super::Out::Give { count: 1, .. }));
        let out = viewer.handle(
            super::Event::Clicked {
                index: 1,
                right: true,
            },
            &ctx,
        );
        assert!(matches!(&out[0], super::Out::Give { count: 99, .. }));
    }

    #[test]
    fn viewer_places_a_known_book_recipe() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[7]),
        );
        let out = viewer.handle(super::Event::Action(0), &context(&[7]));
        assert_eq!(
            out,
            vec![super::Out::Place {
                book_id: 7,
                all: false
            }]
        );
        // The smithing tab's recipe has no book id: its button does nothing.
        viewer.handle(
            super::Event::Key {
                key: "key.keyboard.u".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[]),
        );
        viewer.handle(super::Event::Tab(1), &context(&[]));
        let out = viewer.handle(super::Event::Action(0), &context(&[]));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn language_follows_the_setting() {
        assert_eq!(super::lang(None), super::Lang::En);
        assert_eq!(super::lang(Some("es")), super::Lang::Es);
        assert_eq!(super::lang(Some("fr")), super::Lang::En);
        assert_eq!(super::Lang::En.language_button(), "Language: English");
        assert_eq!(super::Lang::Es.language_button(), "Language: Español");
        assert_eq!(super::Lang::En.give_button(true), "Click gives items: ON");
        assert_eq!(super::Lang::Es.give_button(false), "Clic da objetos: OFF");
    }

    #[test]
    fn viewer_speaks_spanish() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::Es);
        let out = viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:diamond"),
            },
            &context(&[]),
        );
        let super::Out::Recipes { title, views, .. } = &out[0] else {
            panic!("not a page: {out:?}");
        };
        assert_eq!(title, "Recetas: Diamond");
        assert_eq!(views[0].note, "Sin forma");
        let out = viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:stone"),
            },
            &context(&[]),
        );
        assert_eq!(out, vec![super::Out::Say("Sin recetas para: Stone".into())]);
    }

    #[test]
    fn viewer_says_when_there_is_nothing() {
        let (items, recipes) = tables();
        let (mut viewer, _) = super::Viewer::new(items, recipes, super::Lang::En);
        let out = viewer.handle(
            super::Event::Key {
                key: "key.keyboard.r".into(),
                stack: stack("minecraft:stone"),
            },
            &context(&[]),
        );
        assert_eq!(out, vec![super::Out::Say("No recipes for: Stone".into())]);
    }
}
