# Autorun and dashboard

Autorun runs a project's checks without opening OpenCheck, on a schedule. It
reads the project's IFC files, keeps every run in a history and makes a
dashboard that shows how the project develops: what is new and what was
solved since the run before, per pair of disciplines.

## Setting it up

*File → Autorun…* in the app (desktop only):

1. **IFC files**: the files to check, one row each with its discipline and
   the path it is read from. Only these files are checked; other IFC files
   in the same folder are left alone. The list starts with the project's
   models, each read from where it was added from. *Browse…* points a row
   at another file, *Add IFC file…* adds more and *Use the project's
   models* starts the list over. Use UNC paths
   (`\\server\share\project\IFC\…`); drive letters may differ between
   machines. A path with no file behind it is shown in red; a run skips the
   file and says so in the log.
2. **Checks**: the project's rulesets to run. Their settings, the project's
   changes to the information requirements, its LOD level and its IDS files
   are copied into the configuration when it is saved; save again after
   changing them. The visual checks ticked in the Visual Check tab run too
   (see [visual-checking.md](visual-checking.md)).
3. **Output folder**: history, dashboard, log and lock file go here. On the
   network drive, everyone can open the dashboard.
4. **When**: every day or every week, at a time.
5. **Program**: the command line program `opencheck.exe` the task starts.

*Save configuration* writes `autorun.json` and `autorun.bat` into the
output folder; the configuration is also kept in the project. *Run now*
runs once in the background. *Create scheduled task* adds the task to
Windows Task Scheduler on this machine (`schtasks`); change or remove it
there. *Open dashboard* opens the page.

The task runs on the machine it is created on. That machine must be on and
reach the network drive at the time: the user's own computer, or a machine
that is always on.

## A run

`opencheck autorun <folder>\autorun.json` (what `autorun.bat` starts; add
`--force` to run even when nothing changed):

1. **Lock**: `autorun.lock` in the output folder, with the machine name and
   start time, keeps two machines from checking the project at the same
   time. A run that finds the lock skips and logs it; a lock older than 12
   hours was left by a run that stopped and is taken over.
2. **Copying**: each file must keep its size and date for
   `settle_seconds` (10 by default) and be readable, so a file still being
   copied is not checked. A file that does not settle is left out.
3. **Version check**: size and date are compared with the last run
   (`state.json`); only files that differ are read for their SHA-256. When no
   file and no check changed, the run stops there.
4. **Checks**: all chosen rulesets on all files, with the disciplines from
   the configuration, then the visual checks. Their problems become findings
   with the check types `alignment`, `dimension_jump`, `drain_slope` and
   `storey_mismatch`; the log says how many each found.
5. **History**: one file per run, `history/run_YYYY-MM-DD_HHMM.json.gz`,
   never changed afterwards. It holds figures per file (size, IFC schema,
   hash, date, objects in all and per class, elements missing information)
   and every finding (stable id, check type, disciplines, objects with
   GlobalId and class, severity, measured value). To stay small with many
   findings it is gzip-compressed JSON (format 2) made of tables: `rules`
   (ruleset, rule, check type, severity), `objects` (GlobalId, class, name,
   index into `files`, each object once) and `findings`, one row
   `[rule, a, b, value, id]` of indexes per finding; `b` is `a` for a
   finding about one element, and the id is left out when it follows from
   the rule and the two GlobalIds. Plain `.json` files of earlier versions
   are still read.
6. **Dashboard**: `dashboard.html` is written again from the whole history,
   first under a temporary name, then renamed, so nobody opens a half
   written page.
7. **Log**: every step goes to `autorun.log`.

Finding ids stay the same from run to run for the same problem (the rule and
the elements' GlobalIds), so the dashboard can tell new findings from solved
ones.

## The dashboard

One HTML file with the data inside: it opens from a drive in any browser,
without installing anything or reaching the internet.

The page is a preliminary view: it shows the data, but its design is not
finished. The aim is a dashboard that fits one screen without scrolling.

- A choice of check type at the top: all, clashes, clearances, free zones,
  duplicates, information, scripts.
- Findings now, new and solved since the last run, files.
- Discipline against discipline for the latest run: findings, and how many
  are new and solved since the run before.
- Findings per run, in all and for the pairs with the most.
- New and solved per run.
- The files of the latest run: discipline, size, objects and their change,
  elements missing information, IFC schema, last change.
- Objects and file size per file over time.

The history is the record; the page is only a view of it.
