// The lines of the matter refusals whose fix is a folder whose name starts with a dot (words-errors.ts): a matter
// folder holding .claude/scheduled_tasks.json, or a .git folder or file. On the Mac, where Finder hides such folders,
// each line adds how to show them. Apart from words-errors.ts to keep it under the size limit; their Hindi (none
// yet) is in words-errors.hi.ts with the rest. Provisional (app track, via the orchestrator).
import type { ByAgent } from "./words-errors-backslash.ts";

/** matter.claude_tasks_file's lines by the thread's agent (U3-err-matter.claude_tasks_file, .chatgpt, .named,
 * .nameless). */
export const TASKS_FILE: ByAgent = {
  claude:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start Claude in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and Claude can work here.',
  chatgpt:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start ChatGPT in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and ChatGPT can work here.',
  named:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start {name} in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and {name} can work here.',
  nameless:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start the AI in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and the AI can work here.',
};

/** The same on the Mac (each id's .mac). */
export const TASKS_FILE_MAC: ByAgent = {
  claude:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start Claude in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and Claude can work here. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  chatgpt:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start ChatGPT in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and ChatGPT can work here. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  named:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start {name} in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and {name} can work here. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  nameless:
    'This matter\'s folder holds a file named "scheduled_tasks.json", in a folder named ".claude". Clauseo doesn\'t start the AI in a matter that holds it. Move it out of this matter\'s folder, rename it or remove it, and the AI can work here. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
};

/** matter.git_folder's lines by the thread's agent (U3-err-matter.git_folder, .chatgpt, .named, .nameless). */
export const GIT_FOLDER: ByAgent = {
  claude:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start Claude in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and Claude can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder.',
  chatgpt:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start ChatGPT in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and ChatGPT can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder.',
  named:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start {name} in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and {name} can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder.',
  nameless:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start the AI in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and the AI can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder.',
};

/** The same on the Mac (each id's .mac). */
export const GIT_FOLDER_MAC: ByAgent = {
  claude:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start Claude in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and Claude can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  chatgpt:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start ChatGPT in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and ChatGPT can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  named:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start {name} in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and {name} can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
  nameless:
    'This matter\'s folder holds a folder named ".git", which belongs to a code-tracking tool. Clauseo doesn\'t start the AI in a folder that tool tracks. Move the ".git" folder out of this matter\'s folder or rename it, and the AI can work here. If this is a code project you\'re keeping, make a matter from a copy of its files without the ".git" folder. Finder hides folders whose names start with a dot; press Command-Shift-period in Finder to show them.',
};

/** The Mac's lines by code. */
export const DOT_MAC: Readonly<Record<string, ByAgent>> = {
  "matter.claude_tasks_file": TASKS_FILE_MAC,
  "matter.git_folder": GIT_FOLDER_MAC,
};
