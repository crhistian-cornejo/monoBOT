/**
 * Who a group message is for when it names someone with @Name, anywhere in
 * it: one member, the lead, or nobody in particular. Names may hold spaces,
 * so the longest name that matches wins. Pure: no React.
 */

type Person = { id: string; name: string };

export type Addressed<T extends Person> =
  /** Only this member answers, around the lead. */
  | { kind: "member"; member: T; message: string }
  /** Only the lead answers, without a round. */
  | { kind: "lead" };

/** A letter, digit or underscore: what keeps a name going. */
const WORD = /[\p{L}\p{N}_]/u;

const fold = (text: string) =>
  text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** Each @Name in `text`, in order, with where it starts and ends. */
export function mentionsIn<T extends Person>(
  text: string,
  people: readonly T[],
): { person: T; start: number; end: number }[] {
  const named = people
    .filter((person) => person.name.trim())
    .map((person) => ({ person, name: fold(person.name.trim()) }))
    .sort((a, b) => b.name.length - a.name.length);
  const folded = fold(text);
  // Folding removes accents one for one only on text without combining
  // marks; fall back to case alone when lengths differ.
  const haystack = folded.length === text.length ? folded : text.toLowerCase();
  const found: { person: T; start: number; end: number }[] = [];
  for (let at = haystack.indexOf("@"); at !== -1; at = haystack.indexOf("@", at + 1)) {
    // An address (ana@gmail.com) is not a mention.
    if (at > 0 && WORD.test(haystack[at - 1]!)) continue;
    const hit = named.find(({ name }) => {
      const after = haystack[at + 1 + name.length] ?? "";
      return haystack.startsWith(name, at + 1) && !WORD.test(after);
    });
    if (hit)
      found.push({
        person: hit.person,
        start: at,
        end: at + 1 + hit.name.length,
      });
  }
  return found;
}

/**
 * Whom a message names alone. One member: that member answers, and gets the
 * message without its @ («oye @Finance, ¿cuánto gasté?» → «oye Finance,
 * ¿cuánto gasté?»; a leading @Finance is dropped). The lead: the lead alone.
 * Several people, or nobody: undefined, and the whole group takes it.
 */
export function addressedMember<T extends Person>(
  text: string,
  members: readonly T[],
  lead?: Person,
): Addressed<T> | undefined {
  const people: Person[] = lead ? [...members, lead] : [...members];
  const mentions = mentionsIn(text, people);
  const ids = new Set(mentions.map((mention) => mention.person.id));
  if (ids.size !== 1) return undefined;
  const person = mentions[0]!.person;
  if (lead && person.id === lead.id && !members.some((m) => m.id === lead.id))
    return { kind: "lead" };
  let message = "";
  let from = 0;
  for (const mention of mentions) {
    message += text.slice(from, mention.start);
    // A name that opens the message is only the address.
    if (text.slice(0, mention.start).trim()) message += person.name.trim();
    from = mention.end;
  }
  message = (message + text.slice(from)).replace(/^[\s,:;.-]+/, "").trim();
  if (!message) return undefined;
  return { kind: "member", member: person as T, message };
}

/** The people an @ being typed can name, those whose name starts with it first. */
export function rankMentionPeople<T extends Person>(
  people: readonly T[],
  query: string,
): T[] {
  const wanted = fold(query.trim());
  if (!wanted) return [...people];
  const starts: T[] = [];
  const holds: T[] = [];
  for (const person of people) {
    const name = fold(person.name);
    if (name.startsWith(wanted) || name.split(/\s+/).some((word) => word.startsWith(wanted)))
      starts.push(person);
    else if (name.includes(wanted)) holds.push(person);
  }
  return [...starts, ...holds];
}
