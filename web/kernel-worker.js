// The kernel's web worker (docs/ARCHITECTURE.md §7 ter): runs the jobs the UI posts, one at a
// time, in the order they come. In: { id, bytes }. Out: { id, bytes } with the result,
// { id, crashed } when the kernel trapped (a panic: `{ panic }` came just before),
// { fatal } when the kernel could not be loaded at all.
import init, { run } from "./worker/solidsketch_kernel_worker.js";

const ready = init().then(() => true, (err) => {
  self.postMessage({ fatal: String(err) });
  return false;
});

self.onmessage = async (event) => {
  if (!(await ready)) return;
  const { id, bytes } = event.data;
  try {
    const out = run(new Uint8Array(bytes));
    self.postMessage({ id, bytes: out.buffer }, [out.buffer]);
  } catch (err) {
    self.postMessage({ id, crashed: String(err) });
  }
};
