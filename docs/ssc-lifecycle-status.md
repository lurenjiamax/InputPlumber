# Optional SSC lifecycle: experimental

The SSC source owns sensor discovery, polling and retries on a dedicated worker.
A bounded queue carries fresh samples to the composite input loop. Connection
failure must not remove physical input or the virtual controller. Worker shutdown
is signaled without blocking input teardown on synchronous firmware cleanup;
cancellation still depends on the existing libssc synchronous operation behavior.

On AYN Thor, stopping/starting the external motion service preserved the same
InputPlumber process, target paths and SDL attachment. Sensor readings remained
available afterward. The existing SSC stream did not actually disconnect during
that operation, so it does not establish real DSP-loss recovery.

Full platform suspend/reset remains unresolved, including reported failures on
kernel 7.1.2-thorch25. Do not present this source-lifetime change as a verified
system suspend fix. Cold unavailable-SSC startup, true DSP-loss recovery, cached
sensor freshness in consumers and full sleep/wake still require acceptance.
