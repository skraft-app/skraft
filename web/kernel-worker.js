// The kernel's web worker (docs/model/kernel.md): runs the jobs the UI posts, one at a
// time, in the order they come. In: { id, bytes }. Out: { id, progress } while a job runs,
// { id, bytes } with the result, { id, crashed } when the kernel trapped (a panic: `{ panic }`
// came just before), { fatal } when the kernel could not be loaded at all. A job cancelled is
// stopped by the UI terminating this worker.
import init, { run } from "./worker/skraft_kernel_worker.js";

const ready = init().then(() => true, (err) => {
  self.postMessage({ fatal: String(err) });
  return false;
});

self.onmessage = async (event) => {
  if (!(await ready)) return;
  const { id, bytes } = event.data;
  try {
    const out = run(id, new Uint8Array(bytes));
    self.postMessage({ id, bytes: out.buffer }, [out.buffer]);
  } catch (err) {
    self.postMessage({ id, crashed: String(err) });
  }
};
