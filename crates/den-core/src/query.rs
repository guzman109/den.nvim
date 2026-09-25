//! Questions the screens ask, answered as plain data.
//!
//! Every function takes `today` rather than reading the clock, so answers are
//! the same in tests as on any given day.

use jiff::civil::Date;
use serde::Serialize;

use crate::parse::{State, Task};
use crate::vault::{Doc, Kind, Project, ProjectStatus, Vault};

/// Which part of the vault a question is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    /// One project, by name (its file name).
    Project(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectRef {
    pub name: String,
    pub title: String,
}

impl ProjectRef {
    pub(crate) fn of(p: Project<'_>) -> ProjectRef {
        ProjectRef {
            name: p.name().to_string(),
            title: p.title(),
        }
    }
}

/// One task, located in its file and ready to act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskRow {
    pub path: String,
    pub line: usize,
    /// The whole line as read, so an action can tell if it changed since.
    pub raw: String,
    pub text: String,
    pub title: String,
    pub state: State,
    pub tags: Vec<String>,
    pub due: Option<Date>,
    pub done: Option<Date>,
    pub section: Option<String>,
    pub project: Option<ProjectRef>,
    /// The file's title when the task is not in a project file.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskGroup {
    /// `None` for tasks that belong to no project.
    pub project: Option<ProjectRef>,
    pub label: String,
    pub tasks: Vec<TaskRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TasksView {
    /// Everything in progress, across the scope.
    pub doing: Vec<TaskRow>,
    /// Open work by project, not counting inbox captures.
    pub groups: Vec<TaskGroup>,
    /// Done in the last seven days, newest first.
    pub closed: Vec<TaskRow>,
    pub open: usize,
    pub locked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InboxView {
    pub groups: Vec<TaskGroup>,
    pub count: usize,
}

/// One thing the statusline might say, most urgent first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Insight {
    Overdue {
        count: usize,
        first: TaskRow,
    },
    DueToday {
        count: usize,
        first: TaskRow,
    },
    BehindPace {
        project: ProjectRef,
        left: usize,
        days: i64,
    },
    Inbox {
        count: usize,
    },
    DoneToday {
        count: usize,
    },
}

fn is_inbox(task: &Task) -> bool {
    task.section
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("inbox"))
}

impl Vault {
    fn row(&self, doc: &Doc, task: &Task) -> TaskRow {
        TaskRow {
            path: doc.path.clone(),
            line: task.line,
            raw: doc.buf.lines.get(task.line).cloned().unwrap_or_default(),
            text: task.text.clone(),
            title: task.title.clone(),
            state: task.state,
            tags: task.tags.clone(),
            due: task.due,
            done: task.done,
            section: task.section.clone(),
            project: self.project_of(doc).map(ProjectRef::of),
            source: doc.title(),
        }
    }

    /// The documents a scope covers, excluding archived projects (unless the
    /// scope names one) and templates.
    pub(crate) fn scoped_docs(&self, scope: &Scope) -> Vec<&Doc> {
        self.docs()
            .filter(|d| d.kind != Kind::Template && !d.locked)
            .filter(|d| match scope {
                Scope::All => self
                    .project_of(d)
                    .is_none_or(|p| p.status() != ProjectStatus::Archived),
                Scope::Project(name) => self.project_of(d).is_some_and(|p| p.name() == name),
            })
            .collect()
    }

    fn scoped_tasks(&self, scope: &Scope) -> Vec<TaskRow> {
        self.scoped_docs(scope)
            .into_iter()
            .flat_map(|doc| doc.parsed.tasks.iter().map(move |t| self.row(doc, t)))
            .collect()
    }

    pub(crate) fn scoped_projects(&self, scope: &Scope) -> Vec<Project<'_>> {
        let mut projects: Vec<Project<'_>> = self
            .projects()
            .filter(|p| !p.doc.locked)
            .filter(|p| match scope {
                Scope::All => p.status() == ProjectStatus::Active,
                Scope::Project(name) => p.name() == name,
            })
            .collect();
        projects.sort_by_key(|p| p.title().to_lowercase());
        projects
    }

    pub fn tasks_view(&self, scope: &Scope, today: Date) -> TasksView {
        let rows = self.scoped_tasks(scope);
        let mut doing: Vec<TaskRow> = rows
            .iter()
            .filter(|r| r.state == State::Doing)
            .cloned()
            .collect();
        doing.sort_by_key(|r| {
            (
                r.project.as_ref().map(|p| p.title.to_lowercase()),
                r.path.clone(),
                r.line,
            )
        });

        let open_in = |doc: &Doc| -> Vec<TaskRow> {
            doc.parsed
                .tasks
                .iter()
                .filter(|t| t.state == State::Open && !is_inbox(t))
                .map(|t| self.row(doc, t))
                .collect()
        };
        let notes = self.notes_by_project();
        let mut groups = Vec::new();
        for project in self.scoped_projects(scope) {
            let mut tasks = open_in(project.doc);
            for note in notes.get(project.name()).into_iter().flatten() {
                tasks.extend(open_in(note));
            }
            if !tasks.is_empty() {
                groups.push(TaskGroup {
                    label: project.title(),
                    project: Some(ProjectRef::of(project)),
                    tasks,
                });
            }
        }
        if *scope == Scope::All {
            let loose: Vec<TaskRow> = self
                .docs()
                .filter(|d| matches!(d.kind, Kind::Note | Kind::Daily) && !d.locked)
                .filter(|d| self.project_of(d).is_none())
                .flat_map(open_in)
                .collect();
            if !loose.is_empty() {
                groups.push(TaskGroup {
                    project: None,
                    label: "no project".to_string(),
                    tasks: loose,
                });
            }
        }

        let week_start = today.saturating_sub(jiff::Span::new().days(6));
        let mut closed: Vec<TaskRow> = rows
            .iter()
            .filter(|r| r.state == State::Done)
            .filter(|r| r.done.is_some_and(|d| d >= week_start && d <= today))
            .cloned()
            .collect();
        closed.sort_by(|a, b| b.done.cmp(&a.done).then(a.path.cmp(&b.path)));

        let open = doing.len() + groups.iter().map(|g| g.tasks.len()).sum::<usize>();
        let locked = match scope {
            Scope::All => self.projects().filter(|p| p.doc.locked).count(),
            Scope::Project(_) => 0,
        };
        TasksView {
            doing,
            groups,
            closed,
            open,
            locked,
        }
    }

    pub fn inbox_view(&self, scope: &Scope) -> InboxView {
        let pending = |doc: &Doc, all: bool| -> Vec<TaskRow> {
            doc.parsed
                .tasks
                .iter()
                .filter(|t| !t.state.is_closed() && (all || is_inbox(t)))
                .map(|t| self.row(doc, t))
                .collect()
        };
        let notes = self.notes_by_project();
        let mut groups = Vec::new();
        for project in self.scoped_projects(scope) {
            let mut tasks = pending(project.doc, false);
            for note in notes.get(project.name()).into_iter().flatten() {
                tasks.extend(pending(note, false));
            }
            if !tasks.is_empty() {
                groups.push(TaskGroup {
                    label: project.title(),
                    project: Some(ProjectRef::of(project)),
                    tasks,
                });
            }
        }
        if *scope == Scope::All
            && let Some(inbox) = self.docs().find(|d| d.kind == Kind::Inbox && !d.locked)
        {
            let tasks = pending(inbox, true);
            if !tasks.is_empty() {
                groups.push(TaskGroup {
                    project: None,
                    label: "no project".to_string(),
                    tasks,
                });
            }
        }
        let count = groups.iter().map(|g| g.tasks.len()).sum();
        InboxView { groups, count }
    }

    /// What the statusline could say about a scope, most urgent first.
    pub fn insights(&self, scope: &Scope, today: Date) -> Vec<Insight> {
        let rows = self.scoped_tasks(scope);
        let pending: Vec<&TaskRow> = rows.iter().filter(|r| !r.state.is_closed()).collect();
        let mut out = Vec::new();

        let mut overdue: Vec<&&TaskRow> = pending
            .iter()
            .filter(|r| r.due.is_some_and(|d| d < today))
            .collect();
        overdue.sort_by_key(|r| r.due);
        if let Some(first) = overdue.first() {
            out.push(Insight::Overdue {
                count: overdue.len(),
                first: (**first).clone(),
            });
        }
        let due_today: Vec<&&TaskRow> = pending.iter().filter(|r| r.due == Some(today)).collect();
        if let Some(first) = due_today.first() {
            out.push(Insight::DueToday {
                count: due_today.len(),
                first: (**first).clone(),
            });
        }
        for project in self.scoped_projects(scope) {
            if let Some(pace) = self.pace(project, today)
                && pace.behind
            {
                out.push(Insight::BehindPace {
                    project: ProjectRef::of(project),
                    left: pace.left,
                    days: pace.days,
                });
            }
        }
        let inbox = self.inbox_view(scope).count;
        if inbox > 0 {
            out.push(Insight::Inbox { count: inbox });
        }
        let done_today = rows.iter().filter(|r| r.done == Some(today)).count();
        if done_today > 0 {
            out.push(Insight::DoneToday { count: done_today });
        }
        out
    }

    /// Whether a project with an end date will make it at its recent pace.
    ///
    /// Pace is tasks finished per day over the last 14 days. A project is
    /// behind when the work left, at that pace, runs past its `due:` date.
    pub fn pace(&self, project: Project<'_>, today: Date) -> Option<Pace> {
        let due = project.due()?;
        let mut docs = vec![project.doc];
        docs.extend(self.notes_of(project.name()));
        let tasks: Vec<&Task> = docs.iter().flat_map(|d| d.parsed.tasks.iter()).collect();
        let left = tasks.iter().filter(|t| !t.state.is_closed()).count();
        if left == 0 {
            return None;
        }
        let since = today.saturating_sub(jiff::Span::new().days(13));
        let recent = tasks
            .iter()
            .filter(|t| t.state == State::Done)
            .filter(|t| t.done.is_some_and(|d| d >= since && d <= today))
            .count();
        let days = days_between(today, due);
        let per_day = recent as f64 / 14.0;
        let behind = days < 0 || per_day == 0.0 || (left as f64 / per_day) > days as f64;
        Some(Pace {
            left,
            days,
            per_day,
            behind,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pace {
    pub left: usize,
    /// Days until the project's end date; negative once it has passed.
    pub days: i64,
    pub per_day: f64,
    pub behind: bool,
}

/// Whole days from `from` to `to`.
pub fn days_between(from: Date, to: Date) -> i64 {
    from.until(to)
        .ok()
        .map_or(0, |span| i64::from(span.get_days()))
}
