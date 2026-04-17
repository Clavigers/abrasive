import { Callout, Cards, Steps, Tabs, FileTree } from "nextra/components";

export function useMDXComponents(components: Record<string, unknown>) {
  return {
    ...components,
    Callout,
    Cards,
    Steps,
    Tabs,
    FileTree,
  };
}
