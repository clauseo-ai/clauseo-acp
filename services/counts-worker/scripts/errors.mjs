// The deploy scripts' one error type: a step's failure whose message is safe to print in a
// public run log.

/** A step's failure: its message is safe to print. */
export class Refused extends Error {}
