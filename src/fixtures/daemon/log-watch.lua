-- Daemon example: follow a log file and print every line containing "ERROR".
--
-- Run it with the log path passed as state, e.g.:
--   lmb --allow-read /var/log daemon \
--       --file src/fixtures/daemon/log-watch.lua \
--       --state '{"path": "/var/log/app.log"}'
--
-- Pass an optional "limit" to stop after that many lines (used by tests);
-- omit it to follow the log until the daemon receives SIGTERM/SIGINT.
return function(ctx)
    local fs = require("@lmb/fs") -- require INSIDE the returned function

    local opts = ctx.state or {}
    local path = opts.path or error("log-watch: pass --state '{\"path\": \"...\"}'")
    local limit = opts.limit -- optional; nil = run until cancelled

    local seen, matches = 0, 0
    for line in fs:tail(path, { from = "start", poll_interval = 100 }) do
        seen = seen + 1
        if string.find(line, "ERROR") then
            matches = matches + 1
            print(string.format("[match %d @ line %d] %s", matches, seen, line))
        end
        if ctx.cancelled() then -- cooperative graceful shutdown
            break
        end
        if limit and seen >= limit then -- bounded run (tests / one-shot scans)
            break
        end
    end
end
