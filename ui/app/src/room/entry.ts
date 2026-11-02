// What App.tsx mounts: the room, its start, and its stylesheet, styles/index.css, which imports the sheets of the parts
// the room shows.
import "../styles/index.css";

export { Room } from "./room.tsx";
export { connectRoom } from "./start.ts";
