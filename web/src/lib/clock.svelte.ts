// A shared tick for relative times, so many "5 min ago" labels share one timer.

class Clock {
  tick = $state(0);
  /** Unix seconds, refreshed with each tick. */
  now = $state(Date.now() / 1000);

  constructor() {
    setInterval(() => {
      this.tick++;
      this.now = Date.now() / 1000;
    }, 20_000);
  }
}

export const clock = new Clock();
