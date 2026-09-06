export type CatalogParamDto = {
  name: string;
  type: string;
  optional: boolean;
  description?: string | null;
};

export type CatalogFnDto = {
  name: string;
  namespace?: string | null;
  registeredName: string;
  description: string;
  icon: string;
  params: CatalogParamDto[];
  exposure?: string;
};

export type CatalogItemDto = {
  id: string;
  name: string;
  namespace?: string | null;
  registeredName: string;
  description: string;
  icon: string;
  params: CatalogParamDto[];
  tools: string[];
  functions?: CatalogFnDto[];
  exposure?: string;
};

type ToolParamShape = {
  name: string;
  type: string;
  optional?: boolean;
  description?: string;
};

type ToolFunctionShape = {
  name: string;
  namespace?: string;
  registeredName?: string;
  description?: string;
  emoji?: string;
  params?: ToolParamShape[];
  exposure?: string;
};

export type CatalogMergeTarget = {
  id: string;
  params: ToolParamShape[];
  tools?: string[];
  emoji?: string;
  apiDescription?: string;
  functions?: ToolFunctionShape[];
  namespace?: string;
  registeredName?: string;
  exposure?: string;
};

function mapCatalogParam(param: CatalogParamDto): ToolParamShape {
  return {
    name: param.name,
    type: param.type,
    optional: param.optional,
    description: param.description ?? undefined,
  };
}

export function mergeToolCatalog<T extends CatalogMergeTarget>(
  baseTools: readonly T[],
  catalog: CatalogItemDto[],
): T[] {
  const byId = new Map(catalog.map((item) => [item.id, item]));
  return baseTools.map((tool) => {
    const item = byId.get(tool.id);
    if (!item) return tool;

    const params = item.params.map(mapCatalogParam);
    const functions: ToolFunctionShape[] =
      item.functions && item.functions.length > 0
        ? item.functions.map((fn) => ({
            name: fn.name,
            namespace: fn.namespace ?? undefined,
            registeredName: fn.registeredName,
            description: fn.description || undefined,
            emoji: fn.icon || undefined,
            params: fn.params.map(mapCatalogParam),
            exposure: fn.exposure,
          }))
        : (item.tools ?? []).map((name) => ({
            name,
            description:
              name === item.name ? item.description || undefined : undefined,
            emoji: item.icon || undefined,
            params: name === item.name ? params : undefined,
          }));

    return {
      ...tool,
      params,
      tools:
        item.tools.length > 0 ? item.tools : functions.map((fn) => fn.name),
      emoji: item.icon || tool.emoji,
      apiDescription: item.description || tool.apiDescription,
      functions: functions.length > 0 ? functions : tool.functions,
      namespace: item.namespace ?? undefined,
      registeredName: item.registeredName,
      exposure: item.exposure,
    };
  });
}
