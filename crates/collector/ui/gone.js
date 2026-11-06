// Which devices the page shows: those on the network at the moment on show,
// so first heard by then and seen in the week up to it. No DOM, so it's
// tested in node (ui-tests/gone.test.mjs).

export const GONE_AFTER_S = 7 * 86400;

// `at`: the moment, in unix seconds; on the timeline, the end of the bucket on
// show, so a device first heard during that bucket counts. `lastSeen`: when a
// sensor last heard the device or a scan last found it.
export const onNetwork = (devices, at) => devices.filter((d) => d.firstSeen <= at && d.lastSeen >= at - GONE_AFTER_S);
