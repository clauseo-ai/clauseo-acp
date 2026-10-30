import { Field } from "@base-ui/react/field";
import { Input } from "@base-ui/react/input";
import { Toolbar } from "@base-ui/react/toolbar";
import { cn } from "../lib/cn.ts";
import { IconSearch } from "../icons/icons.tsx";
import { searchBox, searchInput } from "./recipes.ts";

/** The search field: set into the ground, 208 by 36, the inset tone. A press anywhere in it focuses the words; focused
 * it turns to the surface tone with the primary outline round the whole field. place: toolbar renders its input
 * through the toolbar, so the arrow keys carry on along it. */
export function SearchField({
  label,
  placeholder = label,
  disabled = false,
  defaultValue,
  place = "alone",
  className,
}: {
  /** Names the field, in the room's words: "Search this matter". */
  label: string;
  placeholder?: string;
  disabled?: boolean;
  defaultValue?: string;
  place?: "alone" | "toolbar";
  className?: string;
}) {
  const input = place === "toolbar" ? <Toolbar.Input /> : <Input />;
  return (
    <Field.Root disabled={disabled} render={<label />} className={cn(searchBox, className)}>
      <IconSearch size={16} />
      <span className="sr-only">{label}</span>
      <Field.Control
        render={input}
        type="search"
        autoComplete="off"
        placeholder={placeholder}
        className={searchInput}
        {...(defaultValue === undefined ? {} : { defaultValue })}
      />
    </Field.Root>
  );
}
