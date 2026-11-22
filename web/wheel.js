// Tells a mouse wheel from a trackpad (docs/ui/interface.md). Browsers report both in pixels
// (deltaMode 0): eframe then hands them as MouseWheelUnit::Point, which the viewport takes for
// a trackpad (orbit). What separates them is not standard and differs by engine.

// A pause longer than this ends a gesture (ms). Safari's fastest wheel events come ~10 ms apart.
const GESTURE_GAP = 200;
// Safari's first event of a wheel turn, whatever the speed: the turns after it are accelerated
// (up to hundreds of pixels), so only a gesture's start can tell.
const SAFARI_NOTCH = 4;

const isPinch = (e) => e.deltaMode !== 0 || e.ctrlKey;

// Chrome: a wheel gives multiples of 120 in wheelDeltaY; a trackpad -3 * deltaY.
const chromeLines = (e) => {
  const w = e.wheelDeltaY;
  if (isPinch(e) || typeof w !== "number" || w === 0 || w % 120 !== 0 || e.deltaX !== 0) return 0;
  return Math.abs(w + 3 * e.deltaY) > 1 ? -w / 120 : 0;
};

// Safari: wheelDeltaY is -3 * deltaY for both, so a gesture is judged by how it starts, and
// keeps that judgement until it pauses. One line per event: its acceleration would otherwise
// jump the zoom. Limit: a trackpad gesture starting at exactly 4 px, straight, reads as a wheel.
const safariLines = () => {
  let last = -Infinity;
  let wheel = false;
  return (e) => {
    if (isPinch(e)) {
      last = -Infinity;
      return 0;
    }
    if (e.timeStamp - last > GESTURE_GAP) {
      // Not exact: macOS gives a hair over it (4.000244), wheelDeltaY being rounded.
      wheel = Math.abs(Math.abs(e.deltaY) - SAFARI_NOTCH) < 0.01 && Math.abs(e.wheelDeltaY + 3 * e.deltaY) < 1;
    }
    last = e.timeStamp;
    if (e.deltaX !== 0) wheel = false;
    return wheel ? Math.sign(e.deltaY) : 0;
  };
};

// A function of a wheel event: the lines it stands for, 0 when it is not a mouse wheel's.
export const wheelLines = (vendor) => (vendor.startsWith("Apple") ? safariLines() : chromeLines);
