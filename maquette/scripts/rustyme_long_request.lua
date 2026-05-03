-- Rustyme LuaJIT hook for long-request fan-out overhead benchmarks.
--
-- Expects kwargs.request_url to point at the local sleep HTTP endpoint
-- (for example http://127.0.0.1:18080/sleep?ms=2000). Returns the server's
-- request_elapsed_ms so the Python harness can compute:
--
--   non_request_ms = end_to_end_ms - request_elapsed_ms

function on_process(envelope)
    local kwargs = envelope.kwargs or {}
    local url = kwargs.request_url or "http://127.0.0.1:18080/sleep?ms=2000"
    local resp = http.get(url, { timeout = kwargs.request_timeout_ms or 30000 })
    if not resp.ok then
        error("http.get failed status=" .. tostring(resp.status))
    end
    local body = json.decode(resp.body)
    local payload = kwargs.payload_b64 or ""
    return {
        ok = true,
        task_id = envelope.id,
        request_url = url,
        request_elapsed_ms = body.request_elapsed_ms,
        server_received_ns = body.server_received_ns,
        sleep_started_ns = body.sleep_started_ns,
        sleep_finished_ns = body.sleep_finished_ns,
        payload_bytes = kwargs.payload_bytes or 0,
        image_b64 = payload,
        format = (payload ~= "") and "png" or nil,
        echo = kwargs,
    }
end
