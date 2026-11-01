// What App.tsx loads in the room build, and only there (VITE_CLAUSEO_WINDOW=room): the room, its start, and its
// stylesheet, styles/index.css, which imports the sheets of the parts the room shows.
import "../styles/index.css";

export { Room } from "./room.tsx";
export { connectRoom } from "./start.ts";
