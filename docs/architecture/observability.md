# Observability

Single-page reference for getting signal out of the running
AnotherOne flutter desktop. Three layers, ordered by leverage:

## 1. Flutter DevTools (browser-based, primary tool for humans)

`flutter run` prints a line near the top of its log:

```
The Flutter DevTools debugger and profiler on Linux is available at:
http://127.0.0.1:<port>/<token>=/devtools/?uri=ws://127.0.0.1:<port>/<token>=/ws
```

Open the `/devtools/?uri=…` URL in any browser. Tabs you'll
actually use:

* **Inspector** — live widget tree. Tap-to-select a rendered
  widget jumps to the source line. Layout Explorer visualises
  `Row` / `Column` constraints, which is the fastest way to
  diagnose `RenderFlex overflowed by Npx` warnings.
* **Logging** — every `developer.log()` event with structured
  filters by name (`aone.titlebar`, `aone.boot`, etc.) and
  level. Errors include the stack-trace pane.
* **Performance** — frame timeline + jank attribution. Wraps
  Flutter's "raster vs build vs paint" budget so you can see
  which phase dropped a frame.
* **Memory** — heap snapshots + leak tracker. Useful when a
  page comes alive faster than it tears down.
* **CPU profiler** — sample-based; click "Record" before
  reproducing a slow path.

DevTools is the first stop for any "why does this look wrong"
or "why is this slow" question.

## 2. App-level logging — `lib/src/log.dart`

Every app log emission goes through three sinks at once:

1. `dart:developer.log()` → DevTools' Logging tab (structured).
2. `stderr.writeln(...)` → `flutter run`'s stdout (one line).
3. Append to `/tmp/aone-debug.log` → tailable file (one line).

Use:

```dart
import 'package:another_one/src/log.dart';

const log = Log('aone.titlebar');

log.info('built chip', {'profile': info.profile});
log.warn('missing branch', {'project': projectId});
log.error('save failed', error: e, stackTrace: s);
```

Naming convention: `aone.<module>` (e.g. `aone.titlebar`,
`aone.right_sidebar.changes`). The `aone.` prefix is what makes
the run log greppable and what DevTools' default filter pins on.

### File tee — tail the app-only stream

```sh
tail -F /tmp/aone-debug.log
```

The file is truncated at every `flutter run` boot (`initLogFile()`
in `main.dart`). One line per event, plus an `error: …` /
stack-trace continuation when an exception is attached. Much
cleaner than the full `flutter run` stdout (which is full of
GTK init noise + Material warnings on every boot).

### Global error sinks

`main.dart` wires three of them:

* `FlutterError.onError` — render / build / layout assertions.
  Logged under `aone.flutter`.
* `PlatformDispatcher.instance.onError` — async errors that
  escape Flutter's zone (e.g. platform-channel callbacks).
  Logged under `aone.platform`.
* `runZonedGuarded` (via `runGuardedApp`) — any unhandled
  async error inside `runApp`. Logged under `aone.uncaught`.

Each one calls `presentError` so the framework's red-screen
stays visible in dev — we add a structured one-line summary,
we don't suppress the original.

## 3. Screenshots — `scripts/aone-screenshot.sh`

`flutter screenshot --type=device` is unsupported on Linux
desktop, and `--type=skia` writes a serialised picture file
(`skiapictm` magic) that ImageMagick can't read. Use Wayland's
native `grim` instead:

```sh
scripts/aone-screenshot.sh             # whole screen
scripts/aone-screenshot.sh --window    # click + drag a region
```

Outputs:
* `/tmp/aone-shot.png` — full-resolution capture.
* `/tmp/aone-shot-1600.png` — same image downscaled to 1600px
  wide. The downscaled file fits comfortably under any
  paste-into-chat size limits.

Requires `grim` (Wayland screenshot tool) and `magick` /
`convert` (ImageMagick). Both are pre-installed on most modern
Linux dev hosts.

## Conventions

* **Levels** map to the Java logger constants Dart standardised
  on: `debug=500`, `info=800`, `warn=900`, `error=1000`. Most
  events should be `info` — reserve `warn` for "something is
  wrong but we recovered" and `error` for "something is wrong
  and the user will see it."
* **Structured fields** (the optional `Map<String, Object?>`
  argument) get JSON-encoded into the line. Use them for ids
  (`projectId`, `tabId`, `agentId`) and counts — anything you'd
  want to grep for without writing a regex.
* **Don't `print()`** — there's an explicit
  `// ignore: avoid_print` exception for the benchmark page's
  machine-readable output line; everything else routes through
  `Log`.
