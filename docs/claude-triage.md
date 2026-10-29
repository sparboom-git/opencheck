# Clash triage with Claude

A model check often gives thousands of findings of which only a few need
action. OpenCheck can let **Claude Desktop** review the findings of the open
project and suggest a category for each by the Danish C402 practice, with how
sure it is and why. You approve, change or dismiss the suggestions in the
Check tab. Claude only reads and suggests: it cannot approve, create issues or
change models and rulesets.

| Category | Meaning | What approving does |
|---|---|---|
| C1 not critical | No effect on quality, cost or time. All noise (agreed and theoretical overlaps) is C1. | The finding is marked *not a problem* (Reject): no action needed |
| C2 critical | Affects quality, cost, time, buildability or construction | An issue with priority C2, the reason in its description |
| C3 very critical | Large effect, to be handled first | An issue with priority C3, the reason in its description |

Every suggestion has a **confidence** (0–1: 0.9 and above is sure, 0.6–0.9
likely, below 0.6 for you to decide) and a short **reason** naming the
elements and systems.

## Set up (once)

1. Install Claude Desktop on the same machine as OpenCheck, and **quit it
   completely** (also its icon in the notification area): it writes its
   settings file over while it runs, so changes made meanwhile are lost.
2. In OpenCheck, *File → Claude connection…*:
   - *Add OpenCheck to Claude Desktop* adds the `opencheck` MCP server to
     Claude Desktop's `claude_desktop_config.json`, keeping its other
     settings; a copy of the file as it was is kept next to it as
     `claude_desktop_config.json.bak`. A file that is not valid JSON is
     left alone and reported. Recent versions keep that file in
     `%LOCALAPPDATA%\Claude-Data\claude_<id>\`, older ones in
     `%APPDATA%\Claude\`; OpenCheck writes to every one it finds. By hand,
     the entry is:

     ```json
     { "mcpServers": { "opencheck": { "command": "C:\\path\\to\\opencheck.exe", "args": ["mcp"] } } }
     ```

   - *Save the triage skill…* saves `opencheck-triage.zip`; add it in Claude
     Desktop under *Settings → Capabilities → Skills*. The skill is the
     method Claude follows; its text is in
     [`skills/opencheck-triage/SKILL.md`](../skills/opencheck-triage/SKILL.md).
3. Start Claude Desktop. *Settings → Developer* lists `opencheck` when it
   has read the entry. Local MCP servers are connected when a chat or task
   starts, so start a new one.

## Review a project

1. Open the project in OpenCheck and run the check.
2. *File → Claude connection… → Allow Claude to connect*. The connection is
   off by default and only listens on this machine (127.0.0.1), with a key
   written to `%APPDATA%\OpenCheck\claude-link.json` while it is on.
3. In Claude Desktop, ask for a review, e.g. *"Gennemgå kollisionerne i
   OpenCheck og kategorisér dem"*.
4. Claude's suggestions show up in the Check tab's *Pre-sort* tab as they
   come in: a tag on each finding (`C2 80%`), the suggestion with its reason
   when you open a finding, and a filter (*With a suggestion*, *Suggested
   noise*, *Suggested C1* …). The *Results* tab shows every finding without
   them, so they do not get in the way of your own sorting.
5. Approve:
   - one finding: *Approve*, or *Approve as* another category;
   - the ticked findings: *Approve suggestions*;
   - in bulk: *Approve* next to the counts, e.g. *Noise, 90 % sure or more*.

   *Dismiss* removes a suggestion; the finding stands without one again.

The dialog lists every call Claude made. *Share element properties* off
keeps property sets from Claude; it then sees classes, names, types, systems
and sizes only.

## How Claude works through the findings

The skill makes Claude start with the groups, not the single findings:
findings with the same rule and the same kinds of elements (class and type
on each side), with a count of the systems and storeys they span. Claude
looks at two or three findings of a group and, when they are alike and the
group lies in one system pair and one storey, rates the whole group at
once; a group that spans several systems or storeys is taken storey by
storey or finding by finding. Every C2 and C3 is looked at on its own. At
the end Claude reports what is C3, which groups were uncertain and what to
look at first.

## Pre-sorting

OpenCheck can mark known noise as C1 by fixed rules before anyone looks:

- insulation (`IfcCovering` named insulation/isolering) around its own pipe
  or duct;
- the same element (same GlobalId) in two models;
- overlaps shallower than a limit (20 mm by default, above the clash rules'
  own 10 mm tolerance);
- a planned opening (`IfcOpeningElement`, or an element named *provision
  for void*, *opening*, *udsparing* or *hulafsætning*) as one of the two
  elements, or a penetration where such an opening is.

Both clashes and elements lying inside another are pre-sorted.

Your own kinds of noise: open a clash and choose *C1 pre-sort* below it,
then *Same types* (e.g. `Basic Wall:VBE250 ↔ Floor:DBEM220`) or *Same
classes* (`IfcWall ↔ IfcSlab`). The kind is kept in the clash rule's
pre-sorting, and the next *Pre-sort* suggests every clash like it as C1, to
approve or not on the *Pre-sort* tab. The Ruleset Manager lists the kinds
under the rule's *Pre-sorting*, where they can be removed.

The first run of a check is raw. Pre-sorting is switched on per clash rule
in the Ruleset Manager (*Pre-sorting*), with each fixed rule and the depth
limit; it is saved in the ruleset and then runs after every check. The
*Pre-sort* button on the *Pre-sort* tab runs it on the results you have, also for
rules where it is off, with the rules' settings as they are now (no need to
run the check again after changing them). Its suggestions are approved like Claude's, and Claude
confirms or corrects them on a sample.

## Learning per project

When you approve a suggestion as another category, the finding (classes,
types, systems, depth, storey), the suggestion and the category you chose are
kept as an example in the project's triage file. Claude reads the file before
every review (`triage_rules`), together with any rules you write for the
project, so the next review follows the project's practice.

## Data

What Claude reads (element and type names, systems, sizes and, if shared,
properties) is sent to Anthropic. Geometry and IFC files are not. Check that
your company's rules and the project allow it before using it on a project.

## The tools

`opencheck mcp` is the MCP server Claude Desktop starts. It holds no models;
each call goes to the running app.

| Tool | What it does |
|---|---|
| `project_info` | Models, results per rule, unrated findings, what C1–C3 mean |
| `triage_rules` | The project's triage rules and corrected examples |
| `list_groups` | Groups of alike findings with depth and examples |
| `list_findings` | Findings one per row, by group, rule, storey or status |
| `get_finding` | One finding in full, with the elements around it |
| `get_element` | One element's properties (if shared) |
| `rate` | Suggestions for findings or whole groups |
| `summary` | Where the review stands |
