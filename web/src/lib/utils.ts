import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

// Purpose: merge conditional class names and resolve Tailwind conflicts.
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}

const plural = new Intl.PluralRules("en");

/** "1 episode", "3 episodes". */
export function count(n: number, one: string, other: string): string {
  return `${n} ${plural.select(n) === "one" ? one : other}`;
}
