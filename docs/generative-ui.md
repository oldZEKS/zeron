# Conversational generative UI

Zeron's `show_visual` tool renders native, sandboxed visuals inline in a conversation. This fork
extends that foundation with reply routing, choice groups, status rows, progress bars, and read-only
diff previews, while keeping controls and local state inside the native visual runtime.

## Native cards

Use `ui.card` for a task-specific section with consistent, theme-aware framing:

```lua
return function()
  return ui.grid { columns = 2, gap = 12,
    ui.card {
      title = "DOM-first",
      description = "Fast and reliable for accessible controls.",
      ui.stat { value = "Low", label = "Setup cost" },
      ui.text "Best for forms, buttons and links.",
    },
    ui.card {
      title = "Visual fallback",
      description = "Useful for canvas-driven or inaccessible interfaces.",
      ui.text "Use screenshots when structured page data is insufficient.",
    },
  }
end
```

Cards use the native renderer and theme. Their children remain ordinary visual nodes, so
charts, stats, tables, tabs and conversational controls can be composed inside them.

## Reply buttons

```lua
return function()
  return ui.column { gap = 8,
    ui.text { "Which approach should I use?", size = "lg", weight = 600 },
    ui.reply {
      label = "DOM-first",
      message = "Use DOM/accessibility targeting first, with screenshot fallback.",
    },
    ui.reply {
      label = "Vision-first",
      message = "Use screenshot-driven visual targeting as the primary strategy.",
    },
  }
end
```

A `ui.reply` is a native button whose bounded `message` is submitted only after the reader
clicks. It cannot execute code or pick arbitrary RPC methods. The desktop sends the response
through the existing durable command plane as a `Steer` command, allowing the agent to receive
the answer during a live turn or on the next turn. The owning device is used for remote chats.
Each button tracks its own in-flight/sent state so submitting one choice doesn't disable the
other controls.

## Choice groups

For a set of mutually exclusive options, use `ui.choices` with top-level state:

```lua
local s = ui.state { approach = "Hybrid" }
return function()
  return ui.column { gap = 12,
    ui.choices {
      title = "Choose an approach",
      description = "DOM-first is efficient; visual fallback covers complex controls.",
      options = { "DOM-first", "Vision-first", "Hybrid" },
      value = s.approach,
      on_change = function(v) s.approach = v end,
      reply_prefix = "Use ",
      reply_suffix = " for browser automation.",
      submit_label = "Continue",
    },
  }
end
```

Changing a tab updates the draft only; Continue sends the selected response. The selected value
must be one of the declared options and the final message is bounded. Keep state in the
script's top-level closure, not inside the rendered view, so choices survive UI rerenders.


## Task status and progress

Use status rows to make the current state explicit without turning the whole answer into a dashboard:

```lua
return function()
  return ui.column { gap = 12,
    ui.status {
      label = "Indexing documents",
      detail = "42 of 60 files processed",
      tone = "info",
    },
    ui.progress {
      label = "Indexing",
      value = 42,
      max = 60,
      tone = "success",
    },
    ui.status {
      label = "Waiting for confirmation",
      detail = "No action will be taken until you choose.",
      tone = "warning",
    },
  }
end
```

`ui.status` accepts `label`, optional `detail`, and `tone` (`info`, `success`,
`warning`, `error`, or `neutral`). `ui.progress` accepts a numeric `value`, optional
`min` and `max` (defaulting to 0 and 100), `label`, `tone`, `segments`, and
`show_value`. Values are clamped to the declared range and rendered as a compact, accessible
text-based bar. These primitives communicate state; they do not prove that a task actually
succeeded. Status must come from observed results, not an optimistic guess.

## Code diff previews

`ui.diff` renders a bounded, read-only preview of proposed changes:

```lua
return function()
  return ui.diff {
    file = "src/retry.rs",
    language = "rust",
    removed = { "let retries = 0;" },
    added = { "let retries = 1;" },
  }
end
```

Use `removed` and `added` arrays of lines. At most 40 lines per side are shown, with a
truncation note when the preview is longer. A diff preview is informational only: it does not
apply patches, open files, run tests, or authorize an operation.


## Structured forms and task steps

Use `ui.form` when a response needs a small set of user-provided values. Keep editable drafts in
`ui.state`, validate them on every render, and pass an explicit boolean `valid`. The submit
button is replaced by a validation status while the form is invalid. The form sends a bounded
message to the conversation only when the user clicks Submit; it does not run shell commands,
apply patches, or invoke application actions.

```lua
local s = ui.state { project = "", budget = "25000" }
return function()
  local amount = tonumber(s.budget)
  local valid = string.match(s.project, "%S") ~= nil and amount ~= nil and amount > 0
  return ui.form {
    title = "Project setup",
    description = "Confirm the details before continuing.",
    fields = {
      ui.text_input {
        label = "Project name",
        value = s.project,
        placeholder = "e.g. Macaneta Tourism School",
        on_change = function(v) s.project = v end,
      },
      ui.number_input {
        label = "Budget (MZN)",
        value = s.budget,
        placeholder = "Enter an amount",
        on_change = function(v) s.budget = v end,
      },
    },
    valid = valid,
    validation_message = "Enter a project name and a positive budget.",
    submit_label = "Submit details",
    message = "Project: " .. s.project .. "\\nBudget (MZN): " .. s.budget,
  }
end
```

A form accepts 1–12 native fields. Validate domain-specific constraints yourself; this is a
presentation and message-composition helper, not a security boundary. The message is still only
user input to the owning conversation.

Use `ui.steps` for a bounded checklist or workflow timeline:

```lua
return function()
  return ui.steps {
    title = "Preparing the release",
    description = "Only mark a step complete after verifying its result.",
    steps = {
      { label = "Inspect changes", detail = "Review the current diff", state = "done" },
      { label = "Run tests", detail = "Waiting for test results", state = "active" },
      { label = "Publish", state = "pending" },
    },
  }
end
```

Step states are `done`, `active`, `pending`, or `blocked`; the list is capped at 12 entries.
These are visual status reports, not proof of completion. Never show a task as done merely because
an action was requested or queued.

## Safety and design rules

- The model chooses whether prose, a visual or an interactive control helps; simple answers stay prose.
- Controls use typed, bounded values. A click never evaluates arbitrary application code.
- A selected answer is user input, not permission to override the user's original instructions.
- Local state stays local unless the user explicitly submits a conversational response.
- A successful enqueue is not proof that the task succeeded; the next agent turn must verify outcomes.
- Existing `ui.button`, charts, custom drawings and 3D scenes remain backwards compatible.

## Next increments

1. Add short text and numeric inputs with local draft state and explicit submit.
2. Add a schema-backed form primitive and richer artifact previews.
3. Bind application actions (open diff, run tests, etc.) to typed permission-checked engine operations.
4. Add action receipts/result states and mobile conversational dispatch parity.
5. Add end-to-end fixtures for duplicate clicks, failed/remote RPC, untrusted content and confirmation gates.

## Scope

This branch implements desktop reply buttons, reusable choice groups, native cards, status rows, progress bars, and read-only code diff previews on top of the existing Luau visual runtime. It is not yet a universal JSON component protocol or
full mobile GenUI. Mobile lays cards out with matching content hierarchy and padding, but the
desktop's outer card frame is not yet rendered there. Conversational reply dispatch is also desktop-only.
