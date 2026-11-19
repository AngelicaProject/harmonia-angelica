use crate::{
    catalog::{Catalog, Loader, Sample},
    coverage::{self, Coverage, Kind},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const ROW_WINDOW: usize = 112;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    pub query: String,
    pub part: Option<Kind>,
    pub marked: Option<bool>,
    pub open: BTreeSet<String>,
    pub first: usize,
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub hash: String,
    pub scope: Option<Vec<String>>,
    pub groups: Vec<(String, Vec<String>)>,
    pub coverage: Coverage,
    pub filter: Filter,
    pub adding: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    #[default]
    None,
    Some,
    All,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expansion {
    Leaf,
    Collapsed,
    Expanded,
    Forced,
}

impl Expansion {
    fn branch(open: bool, forced: bool) -> Self {
        if forced {
            Self::Forced
        } else if open {
            Self::Expanded
        } else {
            Self::Collapsed
        }
    }

    pub(crate) fn open(self) -> bool {
        matches!(self, Self::Expanded | Self::Forced)
    }

    pub(crate) fn can_toggle(self) -> bool {
        matches!(self, Self::Collapsed | Self::Expanded)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Row {
    pub path: String,
    pub title: String,
    pub depth: usize,
    pub folder: bool,
    pub expansion: Expansion,
    pub part: Option<Kind>,
    pub state: State,
    pub quest: Option<String>,
    pub samples: Vec<Sample>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct View {
    pub rows: Vec<Row>,
    pub targets: Vec<String>,
    pub total_rows: usize,
    pub first: usize,
    pub all_marked: bool,
    pub group_marks: BTreeMap<String, State>,
    pub missing: Vec<String>,
    pub candidates: Vec<String>,
    pub candidate_count: usize,
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    cells: u64,
    sheets: usize,
    kept_cells: u64,
    display: Option<Row>,
    parts: Vec<Row>,
    matches: usize,
    marked: usize,
    forced: bool,
}

fn state(kept: u64, total: u64) -> State {
    if kept == 0 {
        State::None
    } else if kept == total {
        State::All
    } else {
        State::Some
    }
}

impl Node {
    fn insert<'a>(
        &'a mut self,
        segments: &[&str],
        cells: u64,
        kept: u64,
        matched: bool,
        marked: bool,
    ) -> &'a mut Self {
        self.sheets += 1;
        self.cells += cells;
        self.kept_cells += kept;
        self.matches += usize::from(matched);
        self.marked += usize::from(matched && marked);
        if let Some((segment, rest)) = segments.split_first() {
            self.children
                .entry((*segment).to_owned())
                .or_default()
                .insert(rest, cells, kept, matched, marked)
        } else {
            self
        }
    }

    fn at_mut(&mut self, path: &str) -> Option<&mut Self> {
        let mut node = self;
        for segment in path.split('/') {
            node = node.children.get_mut(segment)?;
        }
        Some(node)
    }

    fn collect(
        self,
        path: &str,
        depth: usize,
        filter: &Filter,
        visible: bool,
        target: bool,
        view: &mut View,
    ) {
        if self.matches == 0 {
            return;
        }
        let root = path.is_empty();
        let mut child_target = target;
        if target && !root && self.matches == self.sheets && filter.part.is_none() {
            view.targets.push(if self.display.is_some() {
                path.to_owned()
            } else {
                format!("{path}/*")
            });
            child_target = false;
        }
        if let Some(row) = self.display {
            if target && (filter.part.is_some() || self.matches < self.sheets) {
                view.targets.push(path.to_owned());
            }
            if visible {
                let show_parts = row.expansion.open();
                view.rows.push(row);
                if show_parts {
                    view.rows.extend(self.parts);
                }
            }
            return;
        }
        let forced = self.forced
            || (!filter.query.is_empty() && !path.to_lowercase().contains(&filter.query));
        let open = forced || filter.open.contains(path);
        if !root && visible {
            view.rows.push(Row {
                path: path.to_owned(),
                title: path.rsplit('/').next().unwrap_or(path).to_owned(),
                depth,
                folder: true,
                expansion: Expansion::branch(open, forced),
                part: None,
                state: state(self.kept_cells, self.cells),
                quest: None,
                samples: vec![],
            });
        }
        let mut children: Vec<_> = self.children.into_iter().collect();
        children.sort_unstable_by(|(left_name, left), (right_name, right)| {
            let folders = left.display.is_some().cmp(&right.display.is_some());
            folders
                .then_with(|| left_name.to_lowercase().cmp(&right_name.to_lowercase()))
                .then(left_name.cmp(right_name))
        });
        for (name, child) in children {
            let key = if root { name } else { format!("{path}/{name}") };
            child.collect(
                &key,
                depth + usize::from(!root),
                filter,
                visible && (root || open),
                child_target,
                view,
            );
        }
    }
}

pub fn load(request: Request) -> Result<View, String> {
    let bytes =
        crate::library::read_quietly(&crate::catalog::path(&request.hash), 64 * 1024 * 1024)?;
    let mut loader = Loader::new(bytes).ok_or("Invalid sheet catalogue")?;
    let catalog = loader
        .step(usize::MAX)?
        .ok_or("Incomplete sheet catalogue")?;
    build(&catalog, request)
}

#[allow(clippy::too_many_lines)]
pub fn build(catalog: &Catalog, mut request: Request) -> Result<View, String> {
    let filter = &mut request.filter;
    filter.query = filter.query.trim().to_lowercase();
    let legacy = request.coverage.policy_groups();
    let quests: HashMap<_, _> = catalog
        .quests
        .iter()
        .map(|quest| (quest.id.as_str(), quest))
        .collect();
    let mut tree = Node::default();
    let mut view = View::default();
    let mut group_marks: BTreeMap<_, State> = BTreeMap::new();
    let mut present = BTreeSet::new();
    let scope = request.scope.as_ref();

    for sheet in &catalog.sheets {
        let segments: Vec<_> = sheet.name.split('/').collect();
        let cells = u64::from(sheet.cells);
        let covered = scope.is_none_or(|entries| {
            entries
                .iter()
                .any(|entry| coverage::entry_matches(entry, &sheet.name))
        });
        let indices: Vec<_> = sheet.columns.iter().map(|column| column.index).collect();
        let keep = request
            .coverage
            .effective_sheet(&legacy, &sheet.name, &indices, || {
                Ok(sheet
                    .columns
                    .iter()
                    .filter_map(|column| Some((column.name.clone()?, column.index)))
                    .collect())
            })?;
        let kept: u64 = sheet
            .columns
            .iter()
            .filter(|column| keep.keeps(column.index))
            .map(|column| u64::from(column.cells))
            .sum();
        let whole_state = state(kept, cells);
        for (key, entries) in &request.groups {
            if entries
                .iter()
                .any(|entry| coverage::entry_matches(entry, &sheet.name))
            {
                group_marks
                    .entry(key.clone())
                    .and_modify(|previous| {
                        if *previous != whole_state {
                            *previous = State::Some;
                        }
                    })
                    .or_insert(whole_state);
            }
        }
        if !covered {
            tree.insert(&segments, cells, kept, false, false);
            continue;
        }
        if let Some(entries) = scope {
            present.extend(
                entries
                    .iter()
                    .filter(|entry| coverage::entry_matches(entry, &sheet.name)),
            );
        }
        let parts: Vec<_> = [Kind::Name, Kind::Description]
            .into_iter()
            .filter_map(|kind| {
                let columns: Vec<_> = sheet
                    .columns
                    .iter()
                    .filter(|column| coverage::part_kind(&sheet.name, column.index) == Some(kind))
                    .collect();
                if columns.is_empty() {
                    return None;
                }
                let total: u64 = columns.iter().map(|column| u64::from(column.cells)).sum();
                let kept: u64 = columns
                    .iter()
                    .filter(|column| keep.keeps(column.index))
                    .map(|column| u64::from(column.cells))
                    .sum();
                Some((kind, state(kept, total)))
            })
            .collect();
        let filtered_state = filter.part.map_or(Some(whole_state), |kind| {
            parts
                .iter()
                .find(|(part, _)| *part == kind)
                .map(|(_, state)| *state)
        });
        let quest = sheet
            .name
            .strip_prefix("quest/")
            .and_then(|_| segments.last())
            .and_then(|id| quests.get(id));
        let matches_search = filter.query.is_empty()
            || sheet.name.to_lowercase().contains(&filter.query)
            || quest.is_some_and(|quest| {
                [&quest.original, &quest.shown]
                    .iter()
                    .any(|name| name.to_lowercase().contains(&filter.query))
            });
        let matched = matches_search
            && filtered_state.is_some_and(|state| {
                filter.marked.is_none_or(|marked| {
                    if marked {
                        state != State::None
                    } else {
                        state != State::All
                    }
                })
            });
        let node = tree.insert(
            &segments,
            cells,
            kept,
            matched,
            filtered_state == Some(State::All),
        );
        if !matched {
            continue;
        }
        let expansion = if parts.len() > 1 {
            Expansion::branch(filter.open.contains(&sheet.name), filter.part.is_some())
        } else {
            Expansion::Leaf
        };
        let depth = segments.len() - 1;
        node.display = Some(Row {
            path: sheet.name.clone(),
            title: segments.last().unwrap_or(&"").to_string(),
            depth,
            folder: false,
            expansion,
            part: None,
            state: whole_state,
            quest: quest.map(|quest| quest.shown.clone()),
            samples: sheet
                .columns
                .iter()
                .filter_map(|column| column.sample.clone())
                .take(3)
                .collect(),
        });
        node.parts = parts
            .into_iter()
            .filter(|(kind, _)| filter.part.is_none_or(|part| part == *kind))
            .map(|(part, state)| Row {
                path: sheet.name.clone(),
                title: String::new(),
                depth: depth + 1,
                folder: false,
                expansion: Expansion::Leaf,
                part: Some(part),
                state,
                quest: None,
                samples: vec![],
            })
            .collect();
    }
    if let Some(entries) = scope {
        for entry in entries {
            if !present.contains(entry) {
                view.missing.push(entry.clone());
                continue;
            }
            let path = entry
                .split(':')
                .next()
                .unwrap_or(entry)
                .trim_end_matches("/*");
            for (separator, _) in path.match_indices('/') {
                if let Some(parent) = tree.at_mut(&path[..separator]) {
                    parent.forced = true;
                }
            }
        }
    }
    view.group_marks = group_marks;
    view.all_marked = tree.matches > 0 && tree.marked == tree.matches;
    tree.collect("", 0, filter, true, true, &mut view);
    view.total_rows = view.rows.len().min(100_000);
    view.first = filter.first.min(view.total_rows.saturating_sub(1));
    view.rows = view
        .rows
        .into_iter()
        .take(view.total_rows)
        .skip(view.first)
        .take(ROW_WINDOW)
        .collect();
    if let Some(query) = request
        .adding
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        let (found, total) = catalog.find(query, usize::MAX);
        let found: Vec<_> = found
            .into_iter()
            .filter(|entry| scope.is_none_or(|entries| !entries.contains(entry)))
            .collect();
        view.candidate_count = total.min(found.len());
        view.candidates = found.into_iter().take(100).collect();
    }
    Ok(view)
}
