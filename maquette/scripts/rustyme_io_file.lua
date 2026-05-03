-- Rustyme LuaJIT hook for local disk IO fan-out benchmarks.
--
-- Each task writes N MiB to a unique file, flushes/closes, reads it back, then
-- deletes it. This exercises worker-local heavy IO without involving network
-- or external services.

local function mkdir_p(dir)
    os.exec("mkdir -p " .. dir)
end

local function now_ms()
    local out, _err, code = os.exec("date +%s%3N")
    if code ~= 0 then
        return 0
    end
    return tonumber((out:gsub("%s+", ""))) or 0
end

function on_process(envelope)
    local kwargs = envelope.kwargs or {}
    local dir = kwargs.io_dir or "/tmp/rustyme-vs-celery-io"
    local mib = tonumber(kwargs.io_mib or 16)
    mkdir_p(dir)

    local path = dir .. "/rustyme-" .. envelope.id .. ".bin"
    local chunk = string.rep("x", 1024 * 1024)
    local io_started_ms = now_ms()

    local f = assert(io.open(path, "wb"))
    for _ = 1, mib do
        f:write(chunk)
    end
    f:flush()
    f:close()

    local total = 0
    local r = assert(io.open(path, "rb"))
    while true do
        local data = r:read(1024 * 1024)
        if not data then
            break
        end
        total = total + #data
    end
    r:close()
    os.remove(path)

    local io_finished_ms = now_ms()
    return {
        ok = true,
        task_id = envelope.id,
        io_elapsed_ms = io_finished_ms - io_started_ms,
        bytes = total,
        echo = kwargs,
    }
end
