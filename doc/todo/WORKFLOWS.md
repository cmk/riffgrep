-

## Workflow DSL (Lua Scripting)

**cmk:** Yes, I would eventually like to support something akin to SoundMiner's Workflows. I'd like the trait to essentially define an interpreter for a small Workflow DSL.

**LLM:**

### 1. The Workflow DSL Definition

A one-liner might look like:

```text
@vendor:"Black Octopus" | set category "DRUMS" | rename "{recid}_{TIT3}.wav"
```

### 2. The Interpreter Trait

```rust
pub trait WorkflowInterpreter {
    fn parse_workflow(&self, script: &str) -> Result<Vec<WorkflowOp>, ParseError>;
    async fn execute_workflow(
        &self,
        ops: &[WorkflowOp],
        targets: Vec<SearchResult>
    ) -> Result<WorkflowReport, WorkflowError>;
}

pub enum WorkflowOp {
    SetField { field: FieldKey, value: String },
    RegexReplace { field: FieldKey, pattern: String, replacement: String },
    RenameFile { template: String },
    MoveToFolder { path: String },
    ExternalScript { script_path: PathBuf },
}
```

### 3. The "Dry Run"

```rust
impl ActionHandler for WorkflowSource {
    async fn dispatch(&self, action: ActionType, result: &SearchResult) -> Result<(), ActionError> {
        if let ActionType::RunWorkflow(script) = action {
            let ops = self.parse_workflow(&script)?;

            let preview = self.generate_dry_run(&ops, result);

            if self.confirm_preview(preview).await? {
                self.execute_workflow(&ops, vec![result.clone()]).await?;
            }
        }
        Ok(())
    }
}
```

---

## Lua Integration and Diff View

**cmk:** Yes that would be great. Also if you can think of any DSLs that might be appropriate for this use case.

**LLM:**

### Existing DSLs to Consider

- **ExifTool Syntax:** `-Description<TIT3 -Category="DRUMS"`
- **Lua:** Embedding mlua allows users to write actual logic for workflows. Example: `if sample.bpm > 120 then sample.category = "Fast" end`

### The Ratatui "Diff View" Widget

```rust
use ratatui::{widgets::{Block, Borders, Paragraph}, layout::{Layout, Constraint, Direction}};

fn draw_diff_view(f: &mut Frame, area: Rect, original: &UnifiedMetadata, proposed: &UnifiedMetadata) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let before = Paragraph::new(format_meta(original))
        .block(Block::default().title(" BEFORE ").borders(Borders::ALL).fg(Color::Red));

    let after = Paragraph::new(format_meta(proposed))
        .block(Block::default().title(" AFTER (Proposed) ").borders(Borders::ALL).fg(Color::Green));

    f.render_widget(before, chunks[0]);
    f.render_widget(after, chunks[1]);
}

fn format_meta(m: &UnifiedMetadata) -> String {
    format!(
        "TIT3: {}\nTCON: {}\nBPM:  {}\nRating: {}",
        m.description, m.category, m.bpm.unwrap_or(0), m.rating
    )
}
```

---

## Lua/mlua UserData Integration

**cmk:** Yes please (expose your Rust UnifiedMetadata struct to mlua).

**LLM:**

### 1. Implementing UserData for Lua

```rust
use mlua::{UserData, UserDataMethods};

impl UserData for UnifiedMetadata<'_> {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("description", |_, this, (): ()| {
            Ok(this.description.to_string())
        });

        methods.add_method_mut("set_category", |_, this, val: String| {
            this.category = std::borrow::Cow::Owned(val);
            Ok(())
        });

        methods.add_method("bpm", |_, this, (): ()| Ok(this.bpm));
    }
}
```

### 2. The Lua Workflow Interpreter

```rust
pub fn run_lua_workflow(script: &str, mut meta: UnifiedMetadata) -> UnifiedMetadata {
    let lua = mlua::Lua::new();

    lua.scope(|scope| {
        let globals = lua.globals();
        globals.set("sample", scope.create_any_userdata(meta.clone())?)?;
        lua.load(script).exec()?;
        Ok(())
    }).unwrap();

    meta
}
```

### 3. Example User Workflows

**Genre Tagger:**
```lua
if sample:description():find("Kick") then
    sample:set_category("DRUMS")
end
```

**BPM Normalizer:**
```lua
local bpm = sample:bpm()
if bpm and bpm > 170 then
    sample:set_category("Speedcore")
end
```

---

## bpaf --eval CLI Setup

**cmk:** Yes please (bpaf setup for --eval one-liner).

**LLM:**

### 1. bpaf Setup for --eval

```rust
use bpaf::*;

#[derive(Clone, Debug)]
pub struct Options {
    pub eval: Option<String>,
    pub script: Option<std::path::PathBuf>,
}

pub fn options() -> OptionParser<Options> {
    let eval = long("eval")
        .help("Lua one-liner to execute on search results")
        .argument::<String>("CODE")
        .optional();

    let script = long("workflow")
        .help("Path to a .lua workflow file")
        .argument::<std::path::PathBuf>("FILE")
        .optional();

    construct!(Options { eval, script, /* ... */ }).to_options()
}
```

### 2. Integration with Search Results

```rust
fn main() -> Result<()> {
    let opts = options().run();

    if let Some(code) = opts.eval {
        let results = source.search(&opts.query).await?;

        for sample in results {
            let mut meta = source.get_metadata(&sample.path)?;
            let modified = run_lua_workflow(&code, meta)?;
            source.write_metadata(&sample.path, &modified)?;
        }
        return Ok(());
    }

    launch_tui(opts)?;
    Ok(())
}
```

### 3. Example CLI Usage

```bash
# Batch Tagging
sm-tool --eval 'if sample:description():find("Kick") then sample:set_category("DRUMS") end' "./Incoming"

# Property Normalization
sm-tool --eval 'if sample:bpm() > 140 then sample:set_category("Hardcore") end' "./Techno_Pack"

# External Workflows
sm-tool --workflow ./scripts/organize.lua "./Black Octopus"
```

---

## Console-Based Diff Output

**cmk:** That would be great thank you (console-based "Diff" output for CLI safety).

**LLM:**

### 1. Console-Based Diff Logic

```rust
use similar::{ChangeTag, TextDiff};
use console::{style, Style};

pub fn print_meta_diff(original: &UnifiedMetadata, modified: &UnifiedMetadata) {
    let old_text = format_meta_as_list(original);
    let new_text = format_meta_as_list(modified);

    let diff = TextDiff::from_lines(&old_text, &new_text);

    for change in diff.iter_all_changes() {
        let (sign, style) = match change.tag() {
            ChangeTag::Delete => ("-", Style::new().red()),
            ChangeTag::Insert => ("+", Style::new().green()),
            ChangeTag::Equal => (" ", Style::new().dim()),
        };
        print!("{}{}", style.apply_to(sign), style.apply_to(change));
    }
}

fn format_meta_as_list(m: &UnifiedMetadata) -> String {
    format!(
        "TIT3: {}\nTCON: {}\nBPM:  {}\nRating: {}\n",
        m.description, m.category, m.bpm.unwrap_or(0), m.rating
    )
}
```

### 2. The "Safety First" CLI Flow

```rust
if let Some(code) = opts.eval {
    for sample in results {
        let meta = source.get_metadata(&sample.path)?;
        let modified = run_lua_workflow(&code, meta.clone())?;

        if meta != modified {
            println!("{}", style(&sample.path).bold().cyan());
            print_meta_diff(&meta, &modified);

            if !opts.yes_i_know_what_i_am_doing {
                println!("{}", style("Dry run: Use --commit to apply changes.").yellow());
            } else {
                source.write_metadata(&sample.path, &modified)?;
            }
        }
    }
}
```

