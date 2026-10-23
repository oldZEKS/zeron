# Conversational generative UI

Zeron's existing `show_visual` tool already creates native, interactive visuals. This fork
adds an explicit bridge from visible controls to the owning conversation and an ergonomic
choice-group primitive.

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

## Text and number draft inputs

Use native single-line fields when the reader needs to supply a value. Keep the draft in
`ui.state` and handle `on_change`; typing updates local visual state but does not send a chat
message by itself:

```lua
local s = ui.state { project = "", budget = "25000" }
return function()
  return ui.card {
    title = "Project setup",
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
    ui.reply {
      label = "Submit details",
      message = "Project: " .. s.project .. "\\nBudget (MZN): " .. s.budget,
      primary = true,
    },
  }
end
```

Both helpers send `on_change` drafts as strings. That is intentional for number fields: while
typing, a valid draft may temporarily be empty, `-`, or `1.`. Validate with `tonumber(s.budget)`
and check the range before offering a submit button for consequential operations. A `ui.reply`
button is the explicit action that sends a response to the conversation.

Desktop fields use Zeron's native editor, preserving keyboard navigation, caret and selection
between state updates. On mobile the current renderer shows a static value/placeholder preview;
editable touch input is not wired yet.

## Safety and design rules

- The model chooses whether prose, a visual or an interactive control helps; simple answers stay prose.
- Controls use typed, bounded values. A click never evaluates arbitrary application code.
- A selected answer is user input, not permission to override the user's original instructions.
- Local state stays local unless the user explicitly submits a conversational response.
- A successful enqueue is not proof that the task succeeded; the next agent turn must verify outcomes.
- Existing `ui.button`, charts, custom drawings and 3D scenes remain backwards compatible.

## Next increments

1. Add short text and numeric inputs with local draft state and explicit submit.
2. Add schema-backed native components for forms, status, progress, code diffs and artifact previews.
3. Bind application actions (open diff, run tests, etc.) to typed permission-checked engine operations.
4. Add action receipts/result states and mobile conversational dispatch parity.
5. Add end-to-end fixtures for duplicate clicks, failed/remote RPC, untrusted content and confirmation gates.

## Scope

This branch implements desktop reply buttons, reusable choice groups and a native card primitive
on top of the existing Luau visual runtime. It is not yet a universal JSON component protocol or
full mobile GenUI. Mobile lays cards out with matching content hierarchy and padding, but the
desktop's outer card frame is not yet rendered there. Conversational reply dispatch is also desktop-only.
