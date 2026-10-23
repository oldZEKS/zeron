-- The `ui` library every visual script sees. Constructors only tag their
-- props table; Zeron validates and renders the tree natively.
local ui = {}

local function constructor(kind)
	return function(props)
		if type(props) ~= "table" or rawget(props, "__kind") ~= nil then
			props = { props }
		end
		props.__kind = kind
		return props
	end
end

for _, kind in
	{
		"column",
		"row",
		"grid",
		"text",
		"span",
		"rule",
		"space",
		"stat",
		"table",
		"chart",
		"canvas",
		"scene",
		"shader",
		"button",
		"tabs",
		"toggle",
		"slider",
		"link",
	}
do
	ui[kind] = constructor(kind)
end

-- A theme token at an opacity.
function ui.color(token, alpha)
	return { __color = true, token = token, alpha = alpha }
end

-- A blend of two theme tokens, `t` of the way from `a` to `b`.
function ui.mix(a, b, t)
	return { __color = true, token = a, mix = b, t = t }
end

-- The `i`th categorical series color (1-based, cycling through six).
function ui.series(i)
	return "chart" .. ((i - 1) % 6 + 1)
end

-- The table your handlers change. Zeron renders the view again after every
-- handler, so plain assignment is enough.
function ui.state(initial)
	return initial or {}
end

-- A linear map from the domain [d0, d1] onto the range [r0, r1].
function ui.scale(d0, d1, r0, r1)
	local span = d1 - d0
	if span == 0 then
		span = 1
	end
	return function(v)
		return r0 + (v - d0) / span * (r1 - r0)
	end
end

function ui.clamp(v, lo, hi)
	return math.max(lo, math.min(hi, v))
end

function ui.lerp(a, b, t)
	return a + (b - a) * t
end

function ui.map(list, f)
	local out = {}
	for i, v in ipairs(list) do
		out[i] = f(v, i)
	end
	return out
end

function ui.range(a, b, step)
	if b == nil then
		a, b = 1, a
	end
	step = step or 1
	local out = {}
	for v = a, b, step do
		table.insert(out, v)
	end
	return out
end

-- A conversational reply is still a native button, but carries a bounded
-- message that the host submits to the current chat only after the reader clicks.
function ui.reply(props)
	if type(props) ~= "table" then
		error("ui.reply expects a props table")
	end
	props.__kind = "button"
	props.reply = props.message
	props.message = nil
	props.on_click = props.on_click or function() end
	return props
end

-- A native choice group: tabs capture a draft selection, then one reply button
-- sends the selected value to the owning conversation. The state stays in the
-- agent's closure, so re-rendering after a tab change preserves the selection.
function ui.choices(props)
	if type(props) ~= "table" then
		error("ui.choices expects a props table")
	end
	if type(props.options) ~= "table" or #props.options == 0 then
		error("ui.choices requires a non-empty options array")
	end
	if type(props.value) ~= "string" then
		error("ui.choices requires value to be a string")
	end
	local selected = false
	for _, option in ipairs(props.options) do
		if type(option) ~= "string" then
			error("ui.choices options must all be strings")
		end
		if option == props.value then
			selected = true
		end
	end
	if not selected then
		error("ui.choices value must match one of its options")
	end
	if type(props.on_change) ~= "function" then
		error("ui.choices requires on_change")
	end
	local title = props.title
	local description = props.description
	local value = props.value
	local message = (props.reply_prefix or "") .. value .. (props.reply_suffix or "")
	if #message == 0 or #message > 8192 then
		error("ui.choices reply message must contain 1 to 8192 bytes")
	end
	local view = { __kind = "column", gap = props.gap or 12 }
	if title then
		table.insert(view, ui.text { title, size = "lg", weight = 600 })
	end
	if description then
		table.insert(view, ui.text { description, color = "muted" })
	end
	table.insert(view, ui.tabs {
		options = props.options,
		value = value,
		on_change = props.on_change,
	})
	table.insert(view, ui.reply {
		label = props.submit_label or "Continue",
		message = message,
		primary = props.primary ~= false,
	})
	return view
end

return ui
