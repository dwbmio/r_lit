-- Rustyme LuaJIT hook for payload parity benchmarks.
--
-- Mirrors celery_bench_worker.py::bench_echo for payload-specific fields.

function on_process(envelope)
    local kwargs = envelope.kwargs or {}
    if kwargs.results ~= nil then
        return {
            ok = true,
            task_id = envelope.id,
            group_id = kwargs.group_id,
            total = kwargs.total,
            results_count = #kwargs.results,
            results = kwargs.results,
            echo = kwargs,
        }
    end
    return {
        ok = true,
        task_id = envelope.id,
        echo = kwargs,
        payload_bytes = kwargs.payload_bytes or 0,
        image_b64 = kwargs.payload_b64 or "",
        format = (kwargs.payload_b64 and kwargs.payload_b64 ~= "") and "png" or nil,
    }
end
