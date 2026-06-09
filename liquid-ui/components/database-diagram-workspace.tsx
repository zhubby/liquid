"use client";

import {
  type ChangeEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowLeft,
  BoxSelect,
  Download,
  FileJson,
  GitBranch,
  Layers3,
  Loader2,
  Plus,
  RefreshCw,
  Rows3,
  Save,
  Search,
  StickyNote,
  Table2,
  Trash2,
  Upload,
  type LucideIcon,
} from "lucide-react";
import {
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  getBezierPath,
  type Connection,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeChange,
  type NodeProps,
  type OnConnect,
  type OnNodesChange,
} from "@xyflow/react";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
import {
  type CreateDatabaseDiagramRequest,
  type DatabaseDiagram,
  type DatabaseDiagramArea,
  type DatabaseDiagramCardinality,
  type DatabaseDiagramColumn,
  type DatabaseDiagramDocument,
  type DatabaseDiagramEnum,
  type DatabaseDiagramIndex,
  type DatabaseDiagramNote,
  type DatabaseDiagramRelationship,
  type DatabaseDiagramRelationshipEndpoint,
  type DatabaseDiagramTable,
  type UpdateDatabaseDiagramRequest,
  apiRequest,
} from "@/lib/api";
import { useI18n, type Locale } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type DatabaseDiagramWorkspaceProps = {
  token: string;
};

type SelectedElement =
  | { kind: "diagram" }
  | { kind: "table"; id: string }
  | { kind: "relationship"; id: string }
  | { kind: "note"; id: string }
  | { kind: "area"; id: string }
  | { kind: "enum"; id: string };

type DesignCopy = {
  title: string;
  description: string;
  newDiagram: string;
  refresh: string;
  emptyTitle: string;
  emptyDescription: string;
  loadFailed: string;
  createFailed: string;
  loading: string;
  saveFailed: string;
  saved: string;
  importFailed: string;
  imported: string;
  defaultTitle: string;
  updatedAt: string;
  tables: string;
  relationships: string;
  notes: string;
  areas: string;
  enums: string;
  back: string;
  save: string;
  saving: string;
  dirty: string;
  savedState: string;
  addTable: string;
  addRelationship: string;
  addNote: string;
  addArea: string;
  addEnum: string;
  exportJson: string;
  importJson: string;
  search: string;
  canvas: string;
  inspector: string;
  diagram: string;
  table: string;
  relationship: string;
  note: string;
  area: string;
  enum: string;
  name: string;
  schema: string;
  descriptionField: string;
  comment: string;
  color: string;
  position: string;
  size: string;
  width: string;
  height: string;
  columns: string;
  indexes: string;
  addColumn: string;
  addIndex: string;
  dataType: string;
  nullable: string;
  primaryKey: string;
  unique: string;
  defaultValue: string;
  delete: string;
  source: string;
  target: string;
  cardinality: string;
  onUpdate: string;
  onDelete: string;
  body: string;
  values: string;
  addValue: string;
  basicTab: string;
  fieldsTab: string;
  indexesTab: string;
  styleTab: string;
  overviewTab: string;
  documentTab: string;
  endpointsTab: string;
  rulesTab: string;
  noSelection: string;
  relationshipRequirement: string;
  noMatches: string;
};

type TableNodeData = {
  table: DatabaseDiagramTable;
  copy: DesignCopy;
  active: boolean;
  faded: boolean;
};

type NoteNodeData = {
  note: DatabaseDiagramNote;
  copy: DesignCopy;
  active: boolean;
  faded: boolean;
};

type AreaNodeData = {
  area: DatabaseDiagramArea;
  copy: DesignCopy;
  active: boolean;
  faded: boolean;
};

type DiagramNode =
  | Node<TableNodeData, "table">
  | Node<NoteNodeData, "note">
  | Node<AreaNodeData, "area">;

type RelationshipEdgeData = {
  relationship: DatabaseDiagramRelationship;
  active: boolean;
  cardinalityLabel: string;
};

type DiagramEdge = Edge<RelationshipEdgeData, "relationship">;

const designCopy: Record<Locale, DesignCopy> = {
  "zh-CN": {
    title: "数据库设计",
    description: "以结构化文档保存 ER 图、关系、注释和领域分组。",
    newDiagram: "新建设计",
    refresh: "刷新",
    emptyTitle: "暂无数据库设计",
    emptyDescription: "新建设计后会进入画布，可添加表、字段、索引和关系。",
    loadFailed: "加载数据库设计失败",
    createFailed: "创建数据库设计失败",
    loading: "正在加载数据库设计",
    saveFailed: "保存数据库设计失败",
    saved: "数据库设计已保存",
    importFailed: "导入 JSON 失败",
    imported: "设计文档已导入，保存后写入数据库",
    defaultTitle: "未命名数据库设计",
    updatedAt: "更新时间",
    tables: "表",
    relationships: "关系",
    notes: "注释",
    areas: "区域",
    enums: "枚举",
    back: "返回",
    save: "保存",
    saving: "保存中",
    dirty: "未保存",
    savedState: "已保存",
    addTable: "表",
    addRelationship: "关系",
    addNote: "注释",
    addArea: "区域",
    addEnum: "枚举",
    exportJson: "导出 JSON",
    importJson: "导入 JSON",
    search: "搜索表、字段、注释",
    canvas: "设计画布",
    inspector: "属性",
    diagram: "设计",
    table: "表",
    relationship: "关系",
    note: "注释",
    area: "区域",
    enum: "枚举",
    name: "名称",
    schema: "Schema",
    descriptionField: "说明",
    comment: "备注",
    color: "颜色",
    position: "位置",
    size: "尺寸",
    width: "宽",
    height: "高",
    columns: "字段",
    indexes: "索引",
    addColumn: "添加字段",
    addIndex: "添加索引",
    dataType: "类型",
    nullable: "可空",
    primaryKey: "主键",
    unique: "唯一",
    defaultValue: "默认值",
    delete: "删除",
    source: "来源",
    target: "目标",
    cardinality: "基数",
    onUpdate: "ON UPDATE",
    onDelete: "ON DELETE",
    body: "正文",
    values: "值",
    addValue: "添加值",
    basicTab: "基础",
    fieldsTab: "字段",
    indexesTab: "索引",
    styleTab: "样式",
    overviewTab: "概览",
    documentTab: "文档",
    endpointsTab: "端点",
    rulesTab: "规则",
    noSelection: "选择画布元素后编辑属性。",
    relationshipRequirement: "至少需要两个带字段的表才能创建关系。",
    noMatches: "没有匹配的元素。",
  },
  "en-US": {
    title: "Database design",
    description: "Persist ER diagrams, relationships, notes, and domains as structured documents.",
    newDiagram: "New design",
    refresh: "Refresh",
    emptyTitle: "No database designs",
    emptyDescription: "Create a design to open the canvas and add tables, columns, indexes, and relationships.",
    loadFailed: "Failed to load database designs",
    createFailed: "Failed to create database design",
    loading: "Loading database designs",
    saveFailed: "Failed to save database design",
    saved: "Database design saved",
    importFailed: "Failed to import JSON",
    imported: "Design document imported. Save to persist it.",
    defaultTitle: "Untitled database design",
    updatedAt: "Updated",
    tables: "Tables",
    relationships: "Relationships",
    notes: "Notes",
    areas: "Areas",
    enums: "Enums",
    back: "Back",
    save: "Save",
    saving: "Saving",
    dirty: "Unsaved",
    savedState: "Saved",
    addTable: "Table",
    addRelationship: "Relation",
    addNote: "Note",
    addArea: "Area",
    addEnum: "Enum",
    exportJson: "Export JSON",
    importJson: "Import JSON",
    search: "Search tables, columns, notes",
    canvas: "Design canvas",
    inspector: "Inspector",
    diagram: "Design",
    table: "Table",
    relationship: "Relationship",
    note: "Note",
    area: "Area",
    enum: "Enum",
    name: "Name",
    schema: "Schema",
    descriptionField: "Description",
    comment: "Comment",
    color: "Color",
    position: "Position",
    size: "Size",
    width: "Width",
    height: "Height",
    columns: "Columns",
    indexes: "Indexes",
    addColumn: "Add column",
    addIndex: "Add index",
    dataType: "Type",
    nullable: "Nullable",
    primaryKey: "Primary",
    unique: "Unique",
    defaultValue: "Default",
    delete: "Delete",
    source: "Source",
    target: "Target",
    cardinality: "Cardinality",
    onUpdate: "ON UPDATE",
    onDelete: "ON DELETE",
    body: "Body",
    values: "Values",
    addValue: "Add value",
    basicTab: "Basic",
    fieldsTab: "Fields",
    indexesTab: "Indexes",
    styleTab: "Style",
    overviewTab: "Overview",
    documentTab: "Document",
    endpointsTab: "Endpoints",
    rulesTab: "Rules",
    noSelection: "Select a canvas element to edit its properties.",
    relationshipRequirement: "At least two tables with columns are required to create a relationship.",
    noMatches: "No matching elements.",
  },
};

const TABLE_WIDTH = 300;
const NOTE_WIDTH = 288;

const cardinalityOptions: DatabaseDiagramCardinality[] = [
  "one_to_one",
  "one_to_many",
  "many_to_one",
  "many_to_many",
];

export function DatabaseDiagramWorkspace({ token }: DatabaseDiagramWorkspaceProps) {
  const { locale } = useI18n();
  const copy = designCopy[locale];
  const [diagrams, setDiagrams] = useState<DatabaseDiagram[]>([]);
  const [activeDiagram, setActiveDiagram] = useState<DatabaseDiagram | null>(
    null,
  );
  const [isLoading, setIsLoading] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  useEffect(() => {
    let cancelled = false;

    const loadDiagrams = async () => {
      setIsLoading(true);

      try {
        const response = await apiRequest<DatabaseDiagram[]>(
          "/api/v1/database-diagrams",
          { token },
        );

        if (!cancelled) {
          setDiagrams(response);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error(error instanceof Error ? error.message : copy.loadFailed);
          setDiagrams([]);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    };

    void loadDiagrams();

    return () => {
      cancelled = true;
    };
  }, [copy.loadFailed, refreshKey, token]);

  const handleCreateDiagram = async () => {
    if (isCreating) {
      return;
    }

    setIsCreating(true);

    try {
      const body: CreateDatabaseDiagramRequest = {
        title: copy.defaultTitle,
        description: copy.description,
        document: createStarterDocument(),
      };
      const diagram = await apiRequest<DatabaseDiagram>(
        "/api/v1/database-diagrams",
        {
          method: "POST",
          token,
          body,
        },
      );

      setDiagrams((current) => [diagram, ...current]);
      setActiveDiagram(diagram);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : copy.createFailed);
    } finally {
      setIsCreating(false);
    }
  };

  if (activeDiagram) {
    return (
      <div className="fixed inset-0 z-50 bg-background">
        <DatabaseDiagramEditor
          token={token}
          diagram={activeDiagram}
          isFullscreen
          onBack={() => {
            setActiveDiagram(null);
            setRefreshKey((current) => current + 1);
          }}
          onDiagramSaved={(diagram) => {
            setActiveDiagram(diagram);
            setDiagrams((current) =>
              current.map((item) => (item.id === diagram.id ? diagram : item)),
            );
          }}
        />
      </div>
    );
  }

  return (
    <Card className="min-h-[420px] flex-1 rounded-lg py-4 shadow-xs">
      <CardHeader className="flex flex-col gap-3 px-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground shadow-xs">
              <FileJson className="size-4" aria-hidden />
            </span>
            <div className="min-w-0">
              <CardTitle className="truncate text-sm">{copy.title}</CardTitle>
              <p className="mt-1 text-xs text-muted-foreground">
                {copy.description}
              </p>
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isLoading}
            onClick={() => setRefreshKey((current) => current + 1)}
          >
            {isLoading ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="size-4" aria-hidden />
            )}
            {copy.refresh}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={isCreating}
            onClick={() => void handleCreateDiagram()}
          >
            {isCreating ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Plus className="size-4" aria-hidden />
            )}
            {copy.newDiagram}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="px-4">
        {isLoading ? (
          <div className="flex items-center gap-2 rounded-lg border bg-background p-4 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" aria-hidden />
            {copy.loading}
          </div>
        ) : diagrams.length === 0 ? (
          <div className="flex min-h-64 flex-col items-center justify-center rounded-lg border bg-background p-6 text-center">
            <div className="flex size-10 items-center justify-center rounded-md border bg-muted/40">
              <FileJson className="size-5 text-muted-foreground" aria-hidden />
            </div>
            <div className="mt-3 text-sm font-medium">{copy.emptyTitle}</div>
            <p className="mt-1 max-w-sm text-xs leading-5 text-muted-foreground">
              {copy.emptyDescription}
            </p>
          </div>
        ) : (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {diagrams.map((diagram) => (
              <button
                key={diagram.id}
                type="button"
                className="rounded-lg border bg-background p-4 text-left shadow-xs outline-none transition-colors hover:bg-muted/30 focus-visible:ring-[3px] focus-visible:ring-ring/50"
                onClick={() => setActiveDiagram(diagram)}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold">
                      {diagram.title}
                    </div>
                    <p className="mt-1 line-clamp-2 text-xs leading-5 text-muted-foreground">
                      {diagram.description ?? copy.description}
                    </p>
                  </div>
                  <Badge variant="secondary" className="rounded-md">
                    {diagram.document.tables.length}
                  </Badge>
                </div>
                <div className="mt-4 grid grid-cols-4 gap-2 text-xs">
                  <Metric label={copy.tables} value={diagram.document.tables.length} />
                  <Metric
                    label={copy.relationships}
                    value={diagram.document.relationships.length}
                  />
                  <Metric label={copy.notes} value={diagram.document.notes.length} />
                  <Metric label={copy.enums} value={diagram.document.enums.length} />
                </div>
                <div className="mt-3 truncate text-xs text-muted-foreground">
                  {copy.updatedAt}: {formatDateTime(diagram.updated_at, locale)}
                </div>
              </button>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-md border bg-muted/20 px-2 py-1.5">
      <div className="font-mono text-sm font-semibold">{value}</div>
      <div className="mt-0.5 truncate text-muted-foreground">{label}</div>
    </div>
  );
}

function DatabaseDiagramEditor({
  token,
  diagram,
  isFullscreen = false,
  onBack,
  onDiagramSaved,
}: {
  token: string;
  diagram: DatabaseDiagram;
  isFullscreen?: boolean;
  onBack: () => void;
  onDiagramSaved: (diagram: DatabaseDiagram) => void;
}) {
  const { locale } = useI18n();
  const copy = designCopy[locale];
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [title, setTitle] = useState(diagram.title);
  const [description, setDescription] = useState(diagram.description ?? "");
  const [document, setDocument] = useState<DatabaseDiagramDocument>(
    normalizeDocument(diagram.document),
  );
  const [selected, setSelected] = useState<SelectedElement>({ kind: "diagram" });
  const [isSaving, setIsSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    setTitle(diagram.title);
    setDescription(diagram.description ?? "");
    setDocument(normalizeDocument(diagram.document));
    setSelected({ kind: "diagram" });
    setDirty(false);
  }, [diagram]);

  const normalizedQuery = query.trim().toLowerCase();
  const visibleElementIds = useMemo(() => {
    if (!normalizedQuery) {
      return null;
    }

    const ids = new Set<string>();

    for (const table of document.tables) {
      const haystack = [
        table.name,
        table.schema ?? "",
        table.comment ?? "",
        ...table.columns.flatMap((column) => [
          column.name,
          column.data_type,
          column.comment ?? "",
        ]),
      ]
        .join(" ")
        .toLowerCase();

      if (haystack.includes(normalizedQuery)) {
        ids.add(table.id);
      }
    }

    for (const note of document.notes) {
      if (
        [note.title, note.body].join(" ").toLowerCase().includes(normalizedQuery)
      ) {
        ids.add(note.id);
      }
    }

    return ids;
  }, [document.notes, document.tables, normalizedQuery]);

  const selectedCounts = {
    tables: document.tables.length,
    relationships: document.relationships.length,
    notes: document.notes.length,
    areas: document.areas.length,
    enums: document.enums.length,
  };

  const mutateDocument = useCallback((
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => {
    setDocument((current) => normalizeDocument(updater(current)));
    setDirty(true);
  }, []);

  const flowNodes = useMemo(
    () =>
      buildFlowNodes({
        copy,
        document,
        normalizedQuery,
        selected,
        visibleElementIds,
      }),
    [copy, document, normalizedQuery, selected, visibleElementIds],
  );

  const flowEdges = useMemo(
    () => buildFlowEdges(document.relationships, selected),
    [document.relationships, selected],
  );

  const handleNodesChange = useCallback<OnNodesChange<DiagramNode>>(
    (changes) => {
      const positionChanges = changes.filter(isNodePositionChange);

      if (positionChanges.length === 0) {
        return;
      }

      mutateDocument((current) => {
        let nextDocument = current;

        for (const change of positionChanges) {
          const target = selectedElementFromFlowNodeId(change.id);

          if (!target || !change.position) {
            continue;
          }

          const nextPosition = {
            x: Math.round(change.position.x),
            y: Math.round(change.position.y),
          };

          if (target.kind === "table") {
            nextDocument = {
              ...nextDocument,
              tables: nextDocument.tables.map((table) =>
                table.id === target.id
                  ? { ...table, position: nextPosition }
                  : table,
              ),
            };
          } else if (target.kind === "note") {
            nextDocument = {
              ...nextDocument,
              notes: nextDocument.notes.map((note) =>
                note.id === target.id ? { ...note, position: nextPosition } : note,
              ),
            };
          } else if (target.kind === "area") {
            nextDocument = {
              ...nextDocument,
              areas: nextDocument.areas.map((area) =>
                area.id === target.id ? { ...area, position: nextPosition } : area,
              ),
            };
          }
        }

        return nextDocument;
      });
    },
    [mutateDocument],
  );

  const handleConnect = useCallback<OnConnect>(
    (connection) => {
      const relationship = relationshipFromConnection(document, connection);

      if (!relationship) {
        return;
      }

      mutateDocument((current) => ({
        ...current,
        relationships: [...current.relationships, relationship],
      }));
      setSelected({ kind: "relationship", id: relationship.id });
    },
    [document, mutateDocument],
  );

  const handleTitleChange = (value: string) => {
    setTitle(value);
    setDirty(true);
  };

  const handleDescriptionChange = (value: string) => {
    setDescription(value);
    setDirty(true);
  };

  const handleSave = async () => {
    if (isSaving) {
      return;
    }

    setIsSaving(true);

    try {
      const documentForSave = prepareDocumentForSave(document);
      const body: UpdateDatabaseDiagramRequest = {
        title,
        description,
        document: documentForSave,
      };
      const saved = await apiRequest<DatabaseDiagram>(
        `/api/v1/database-diagrams/${diagram.id}`,
        {
          method: "PATCH",
          token,
          body,
        },
      );

      setDirty(false);
      setTitle(saved.title);
      setDescription(saved.description ?? "");
      setDocument(normalizeDocument(saved.document));
      onDiagramSaved(saved);
      toast.success(copy.saved);
    } catch (error) {
      toast.error(error instanceof Error ? error.message : copy.saveFailed);
    } finally {
      setIsSaving(false);
    }
  };

  const addTable = () => {
    const tableNumber = document.tables.length + 1;
    const tableId = createId("table");
    const columnId = createId("column");
    const table: DatabaseDiagramTable = {
      id: tableId,
      name: `table_${tableNumber}`,
      schema: "public",
      position: {
        x: 180 + ((tableNumber - 1) % 3) * 440,
        y: 180 + Math.floor((tableNumber - 1) / 3) * 300,
      },
      color: "#2563eb",
      comment: undefined,
      columns: [
        {
          id: columnId,
          name: "id",
          data_type: "uuid",
          nullable: false,
          primary_key: true,
          unique: true,
          default_value: undefined,
          comment: undefined,
        },
      ],
      indexes: [],
    };

    mutateDocument((current) => ({
      ...current,
      tables: [...current.tables, table],
    }));
    setSelected({ kind: "table", id: tableId });
  };

  const addRelationship = () => {
    const candidates = document.tables.filter((table) => table.columns.length > 0);

    if (candidates.length < 2) {
      toast.error(copy.relationshipRequirement);
      return;
    }

    const sourceTable = candidates[0];
    const targetTable = candidates[1];
    const relationship: DatabaseDiagramRelationship = {
      id: createId("relationship"),
      name: `${sourceTable.name}_${targetTable.name}_fk`,
      source: endpointFor(sourceTable, sourceTable.columns[0]),
      target: endpointFor(targetTable, targetTable.columns[0]),
      cardinality: "many_to_one",
      on_update: "no_action",
      on_delete: "no_action",
    };

    mutateDocument((current) => ({
      ...current,
      relationships: [...current.relationships, relationship],
    }));
    setSelected({ kind: "relationship", id: relationship.id });
  };

  const addNote = () => {
    const note: DatabaseDiagramNote = {
      id: createId("note"),
      title: `${copy.note} ${document.notes.length + 1}`,
      body: "",
      position: {
        x: 520 + document.notes.length * 32,
        y: 180 + document.notes.length * 32,
      },
    };

    mutateDocument((current) => ({
      ...current,
      notes: [...current.notes, note],
    }));
    setSelected({ kind: "note", id: note.id });
  };

  const addArea = () => {
    const area: DatabaseDiagramArea = {
      id: createId("area"),
      title: `${copy.area} ${document.areas.length + 1}`,
      position: {
        x: 120 + document.areas.length * 40,
        y: 120 + document.areas.length * 40,
      },
      size: {
        width: 560,
        height: 340,
      },
      color: "#dbeafe",
    };

    mutateDocument((current) => ({
      ...current,
      areas: [...current.areas, area],
    }));
    setSelected({ kind: "area", id: area.id });
  };

  const addEnum = () => {
    const enumItem: DatabaseDiagramEnum = {
      id: createId("enum"),
      name: `enum_${document.enums.length + 1}`,
      values: [
        {
          id: createId("enum_value"),
          name: "pending",
          comment: undefined,
        },
      ],
    };

    mutateDocument((current) => ({
      ...current,
      enums: [...current.enums, enumItem],
    }));
    setSelected({ kind: "enum", id: enumItem.id });
  };

  const exportJson = () => {
    const payload = JSON.stringify(
      {
        title,
        description: description || undefined,
        document,
      },
      null,
      2,
    );
    const url = URL.createObjectURL(
      new Blob([payload], { type: "application/json" }),
    );
    const link = window.document.createElement("a");
    link.href = url;
    link.download = `${slugify(title || copy.defaultTitle)}.database-diagram.json`;
    link.click();
    URL.revokeObjectURL(url);
  };

  const importJson = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";

    if (!file) {
      return;
    }

    try {
      const text = await file.text();
      const payload = JSON.parse(text) as {
        title?: string;
        description?: string;
        document?: DatabaseDiagramDocument;
      };
      const nextDocument = normalizeDocument(payload.document ?? payload as DatabaseDiagramDocument);

      if (payload.title) {
        setTitle(payload.title);
      }

      if (payload.description !== undefined) {
        setDescription(payload.description);
      }

      setDocument(nextDocument);
      setDirty(true);
      setSelected({ kind: "diagram" });
      toast.success(copy.imported);
    } catch {
      toast.error(copy.importFailed);
    }
  };

  const matchedSomething =
    !visibleElementIds ||
    visibleElementIds.size > 0 ||
    document.areas.some((area) =>
      area.title.toLowerCase().includes(normalizedQuery),
    ) ||
    document.enums.some((enumItem) =>
      enumItem.name.toLowerCase().includes(normalizedQuery),
    );

  return (
    <section
      className={cn(
        "flex flex-1 flex-col overflow-hidden bg-background",
        isFullscreen
          ? "h-screen rounded-none border-0 shadow-none"
          : "min-h-[calc(100vh-1.5rem)] rounded-lg border shadow-xs",
      )}
    >
      <header className="flex flex-col gap-3 border-b bg-card px-3 py-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex min-w-0 items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={copy.back}
            title={copy.back}
            onClick={onBack}
          >
            <ArrowLeft className="size-4" aria-hidden />
          </Button>
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <input
                value={title}
                className="h-8 min-w-0 max-w-96 rounded-md border bg-background px-2 text-sm font-semibold outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                aria-label={copy.name}
                onChange={(event) => handleTitleChange(event.target.value)}
              />
              <Badge variant={dirty ? "outline" : "secondary"} className="rounded-md">
                {dirty ? copy.dirty : copy.savedState}
              </Badge>
            </div>
            <div className="mt-1 truncate text-xs text-muted-foreground">
              {copy.updatedAt}: {formatDateTime(diagram.updated_at, locale)}
            </div>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ToolbarButton icon={Table2} label={copy.addTable} onClick={addTable} />
          <ToolbarButton
            icon={GitBranch}
            label={copy.addRelationship}
            onClick={addRelationship}
          />
          <ToolbarButton icon={StickyNote} label={copy.addNote} onClick={addNote} />
          <ToolbarButton icon={BoxSelect} label={copy.addArea} onClick={addArea} />
          <ToolbarButton icon={Layers3} label={copy.addEnum} onClick={addEnum} />
          <div className="mx-1 hidden h-6 w-px bg-border sm:block" />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => fileInputRef.current?.click()}
          >
            <Upload className="size-4" aria-hidden />
            {copy.importJson}
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={exportJson}>
            <Download className="size-4" aria-hidden />
            {copy.exportJson}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={isSaving}
            onClick={() => void handleSave()}
          >
            {isSaving ? (
              <Loader2 className="size-4 animate-spin" aria-hidden />
            ) : (
              <Save className="size-4" aria-hidden />
            )}
            {isSaving ? copy.saving : copy.save}
          </Button>
          <input
            ref={fileInputRef}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={importJson}
          />
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-[minmax(320px,1fr)_minmax(260px,34vh)] lg:grid-cols-[minmax(0,1fr)_360px] lg:grid-rows-1">
        <section className="flex min-h-0 min-w-0 flex-col border-b bg-muted/20 lg:border-b-0 lg:border-r">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b bg-background px-3 py-2">
            <div className="relative min-w-64 flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <input
                value={query}
                placeholder={copy.search}
                className="h-9 w-full rounded-md border bg-background pl-9 pr-3 text-sm outline-none placeholder:text-muted-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50"
                onChange={(event) => setQuery(event.target.value)}
              />
            </div>
            <div className="flex flex-wrap gap-1.5 text-xs">
              <Badge variant="outline" className="rounded-md">
                {copy.tables}: {selectedCounts.tables}
              </Badge>
              <Badge variant="outline" className="rounded-md">
                {copy.relationships}: {selectedCounts.relationships}
              </Badge>
              <Badge variant="outline" className="rounded-md">
                {copy.enums}: {selectedCounts.enums}
              </Badge>
            </div>
          </div>

          <div className="relative min-h-0 flex-1 bg-muted/30">
            {!matchedSomething ? (
              <div className="absolute left-3 top-3 z-20 rounded-md border bg-background px-3 py-2 text-xs text-muted-foreground shadow-sm">
                {copy.noMatches}
              </div>
            ) : null}
            <ReactFlow
              nodes={flowNodes}
              edges={flowEdges}
              nodeTypes={diagramNodeTypes}
              edgeTypes={diagramEdgeTypes}
              onNodesChange={handleNodesChange}
              onConnect={handleConnect}
              onNodeClick={(_, node) => {
                const nextSelected = selectedElementFromFlowNodeId(node.id);

                if (nextSelected) {
                  setSelected(nextSelected);
                }
              }}
              onEdgeClick={(_, edge) =>
                setSelected({ kind: "relationship", id: edge.id })
              }
              onPaneClick={() => setSelected({ kind: "diagram" })}
              fitView
              fitViewOptions={{ maxZoom: 1, padding: 0.2 }}
              minZoom={0.25}
              maxZoom={1.4}
              nodesDraggable
              nodesConnectable
              elementsSelectable
              snapToGrid
              snapGrid={[16, 16]}
              proOptions={{ hideAttribution: true }}
              className="database-diagram-flow"
            >
              <Background gap={28} size={1} />
              <Controls position="bottom-left" />
              <MiniMap
                position="bottom-right"
                pannable
                zoomable
                nodeStrokeWidth={2}
                nodeColor={(node) =>
                  node.type === "table"
                    ? "var(--primary)"
                    : "var(--muted-foreground)"
                }
              />
            </ReactFlow>
          </div>
        </section>

        <InspectorPanel
          copy={copy}
          title={title}
          description={description}
          document={document}
          selected={selected}
          onTitleChange={handleTitleChange}
          onDescriptionChange={handleDescriptionChange}
          onSelect={setSelected}
          mutateDocument={mutateDocument}
        />
      </div>
    </section>
  );
}

function ToolbarButton({
  icon: Icon,
  label,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button type="button" variant="outline" size="sm" onClick={onClick}>
      <Icon className="size-4" aria-hidden />
      {label}
    </Button>
  );
}

function FlowTableNode({
  data,
  selected,
  isConnectable,
}: NodeProps<Extract<DiagramNode, { type: "table" }>>) {
  const { table, copy, faded } = data;
  const active = data.active || selected;

  return (
    <article
      className={cn(
        "database-diagram-node database-diagram-table-node overflow-hidden rounded-lg border bg-card text-card-foreground transition",
        active && "border-primary ring-2 ring-primary/25",
        faded && "opacity-25",
      )}
      style={{ width: TABLE_WIDTH }}
    >
      <div
        className="h-1.5"
        style={{ backgroundColor: table.color ?? "var(--primary)" }}
      />
      <div className="border-b bg-muted/20 px-3 py-2.5">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex min-w-0 items-center gap-2">
              <span className="flex size-6 shrink-0 items-center justify-center rounded-md border bg-background">
                <Table2 className="size-3.5 text-muted-foreground" aria-hidden />
              </span>
              <div className="truncate text-sm font-semibold">{table.name}</div>
            </div>
            <div className="mt-1.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
              <span className="truncate">{table.schema ?? "public"}</span>
              <span aria-hidden>·</span>
              <span>{table.columns.length} {copy.columns}</span>
            </div>
          </div>
          <div className="flex shrink-0 flex-col items-end gap-1">
            <Badge variant="secondary" className="rounded-md px-1.5 py-0 text-[10px]">
              {table.indexes.length} IDX
            </Badge>
            {table.columns.some((column) => column.primary_key) ? (
              <Badge variant="outline" className="rounded-md px-1.5 py-0 text-[10px]">
                PK
              </Badge>
            ) : null}
          </div>
        </div>
        {table.comment ? (
          <p className="mt-2 line-clamp-2 text-xs leading-5 text-muted-foreground">
            {table.comment}
          </p>
        ) : null}
      </div>
      <div className="divide-y">
        {table.columns.length === 0 ? (
          <div className="px-3 py-3 text-xs text-muted-foreground">
            {copy.columns}: 0
          </div>
        ) : (
          table.columns.map((column) => (
            <div
              key={column.id}
              className="relative grid grid-cols-[minmax(0,1fr)_auto] gap-3 px-3 py-2 text-xs"
            >
              <Handle
                type="target"
                position={Position.Left}
                id={targetHandleId(column.id)}
                isConnectable={isConnectable}
                className="database-diagram-handle database-diagram-handle-left"
              />
              <div className="min-w-0">
                <div className="flex min-w-0 items-center gap-1.5">
                  {column.primary_key ? (
                    <span className="rounded-sm bg-primary px-1.5 py-0.5 text-[10px] font-semibold text-primary-foreground">
                      PK
                    </span>
                  ) : null}
                  {column.unique && !column.primary_key ? (
                    <span className="rounded-sm border px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
                      UQ
                    </span>
                  ) : null}
                  <span className="truncate font-medium">{column.name}</span>
                </div>
                <div className="mt-1 truncate text-[11px] text-muted-foreground">
                  {column.nullable ? "nullable" : "not null"}
                  {column.default_value ? ` · ${column.default_value}` : ""}
                </div>
              </div>
              <div className="self-start rounded-md bg-muted px-1.5 py-1 font-mono text-[11px] text-muted-foreground">
                {column.data_type}
              </div>
              <Handle
                type="source"
                position={Position.Right}
                id={sourceHandleId(column.id)}
                isConnectable={isConnectable}
                className="database-diagram-handle database-diagram-handle-right"
              />
            </div>
          ))
        )}
      </div>
    </article>
  );
}

function FlowNoteNode({
  data,
  selected,
}: NodeProps<Extract<DiagramNode, { type: "note" }>>) {
  const { note, copy, faded } = data;
  const active = data.active || selected;

  return (
    <article
      className={cn(
        "database-diagram-node w-72 rounded-lg border bg-card p-3 text-card-foreground transition",
        active && "border-primary ring-2 ring-primary/25",
        faded && "opacity-25",
      )}
      style={{ width: NOTE_WIDTH }}
    >
      <div className="flex items-center gap-2 text-sm font-semibold">
        <span className="flex size-6 shrink-0 items-center justify-center rounded-md border bg-muted/40">
          <StickyNote className="size-3.5 text-muted-foreground" aria-hidden />
        </span>
        <span className="truncate">{note.title}</span>
      </div>
      <p className="mt-2 line-clamp-5 whitespace-pre-wrap text-xs leading-5 text-muted-foreground">
        {note.body || copy.note}
      </p>
    </article>
  );
}

function FlowAreaNode({
  data,
  selected,
}: NodeProps<Extract<DiagramNode, { type: "area" }>>) {
  const { area, copy, faded } = data;
  const active = data.active || selected;

  return (
    <section
      className={cn(
        "database-diagram-area-node rounded-lg border-2 border-dashed p-3 text-xs transition",
        active ? "border-primary" : "border-border",
        faded && "opacity-25",
      )}
      style={{
        width: area.size.width,
        height: area.size.height,
        backgroundColor: area.color ?? "var(--muted)",
      }}
    >
      <div className="truncate font-semibold text-foreground">{area.title}</div>
      <div className="mt-1 text-muted-foreground">
        {copy.size}: {area.size.width} x {area.size.height}
      </div>
    </section>
  );
}

function RelationshipEdge(props: EdgeProps<DiagramEdge>) {
  const {
    id,
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    markerEnd,
    selected,
    style,
    data,
  } = props;
  const active = Boolean(selected || data?.active);
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });

  return (
    <>
      <BaseEdge
        id={id}
        path={edgePath}
        markerEnd={markerEnd}
        interactionWidth={22}
        style={{
          ...style,
          stroke: active ? "var(--primary)" : "var(--muted-foreground)",
          strokeWidth: active ? 2.5 : 1.75,
        }}
      />
      <EdgeLabelRenderer>
        <div
          className={cn(
            "nodrag nopan pointer-events-none absolute rounded-md border bg-background px-2 py-1 text-[11px] font-medium shadow-xs",
            active ? "border-primary text-foreground" : "text-muted-foreground",
          )}
          style={{
            transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
          }}
        >
          <span className="mr-1 font-mono">{data?.cardinalityLabel}</span>
          {data?.relationship.name}
        </div>
      </EdgeLabelRenderer>
    </>
  );
}

const diagramNodeTypes = {
  table: FlowTableNode,
  note: FlowNoteNode,
  area: FlowAreaNode,
};

const diagramEdgeTypes = {
  relationship: RelationshipEdge,
};

function buildFlowNodes({
  copy,
  document,
  normalizedQuery,
  selected,
  visibleElementIds,
}: {
  copy: DesignCopy;
  document: DatabaseDiagramDocument;
  normalizedQuery: string;
  selected: SelectedElement;
  visibleElementIds: Set<string> | null;
}): DiagramNode[] {
  return [
    ...document.areas.map(
      (area): DiagramNode => ({
        id: flowNodeId("area", area.id),
        type: "area",
        position: area.position,
        data: {
          area,
          copy,
          active: selected.kind === "area" && selected.id === area.id,
          faded: Boolean(
            normalizedQuery &&
              !area.title.toLowerCase().includes(normalizedQuery),
          ),
        },
        draggable: true,
        selectable: true,
        zIndex: 0,
      }),
    ),
    ...document.tables.map(
      (table): DiagramNode => ({
        id: flowNodeId("table", table.id),
        type: "table",
        position: table.position,
        data: {
          table,
          copy,
          active: selected.kind === "table" && selected.id === table.id,
          faded: Boolean(
            visibleElementIds &&
              normalizedQuery &&
              !visibleElementIds.has(table.id),
          ),
        },
        draggable: true,
        selectable: true,
        zIndex: 20,
      }),
    ),
    ...document.notes.map(
      (note): DiagramNode => ({
        id: flowNodeId("note", note.id),
        type: "note",
        position: note.position,
        data: {
          note,
          copy,
          active: selected.kind === "note" && selected.id === note.id,
          faded: Boolean(
            visibleElementIds &&
              normalizedQuery &&
              !visibleElementIds.has(note.id),
          ),
        },
        draggable: true,
        selectable: true,
        zIndex: 30,
      }),
    ),
  ];
}

function buildFlowEdges(
  relationships: DatabaseDiagramRelationship[],
  selected: SelectedElement,
): DiagramEdge[] {
  return relationships.map((relationship) => {
    const active =
      selected.kind === "relationship" && selected.id === relationship.id;

    return {
      id: relationship.id,
      type: "relationship",
      source: flowNodeId("table", relationship.source.table_id),
      target: flowNodeId("table", relationship.target.table_id),
      sourceHandle: sourceHandleId(relationship.source.column_id),
      targetHandle: targetHandleId(relationship.target.column_id),
      selected: active,
      data: {
        relationship,
        active,
        cardinalityLabel: cardinalityLabel(relationship.cardinality),
      },
      markerEnd: {
        type: MarkerType.ArrowClosed,
        width: 18,
        height: 18,
        color: active
          ? "var(--primary)"
          : "var(--muted-foreground)",
      },
      style: {
        stroke: active
          ? "var(--primary)"
          : "var(--muted-foreground)",
      },
      zIndex: active ? 40 : 10,
    };
  });
}

function relationshipFromConnection(
  document: DatabaseDiagramDocument,
  connection: Connection,
): DatabaseDiagramRelationship | null {
  const sourceTableId = flowTableIdFromNode(connection.source);
  const targetTableId = flowTableIdFromNode(connection.target);
  const sourceColumnId = columnIdFromSourceHandle(connection.sourceHandle);
  const targetColumnId = columnIdFromTargetHandle(connection.targetHandle);

  if (!sourceTableId || !targetTableId || !sourceColumnId || !targetColumnId) {
    return null;
  }

  if (sourceTableId === targetTableId && sourceColumnId === targetColumnId) {
    return null;
  }

  const sourceTable = document.tables.find((table) => table.id === sourceTableId);
  const targetTable = document.tables.find((table) => table.id === targetTableId);
  const sourceColumn = sourceTable?.columns.find(
    (column) => column.id === sourceColumnId,
  );
  const targetColumn = targetTable?.columns.find(
    (column) => column.id === targetColumnId,
  );

  if (!sourceTable || !targetTable || !sourceColumn || !targetColumn) {
    return null;
  }

  return {
    id: createId("relationship"),
    name: `${sourceTable.name}_${targetTable.name}_fk`,
    source: endpointFor(sourceTable, sourceColumn),
    target: endpointFor(targetTable, targetColumn),
    cardinality: "many_to_one",
    on_update: "no_action",
    on_delete: "no_action",
  };
}

function flowNodeId(kind: "table" | "note" | "area", id: string) {
  return `${kind}:${id}`;
}

function flowTableIdFromNode(nodeId: string | null): string | null {
  if (!nodeId?.startsWith("table:")) {
    return null;
  }

  return nodeId.slice("table:".length);
}

function selectedElementFromFlowNodeId(nodeId: string): SelectedElement | null {
  const separator = nodeId.indexOf(":");

  if (separator < 0) {
    return null;
  }

  const kind = nodeId.slice(0, separator);
  const id = nodeId.slice(separator + 1);

  if (kind === "table" || kind === "note" || kind === "area") {
    return { kind, id };
  }

  return null;
}

function sourceHandleId(columnId: string) {
  return `source-column:${columnId}`;
}

function targetHandleId(columnId: string) {
  return `target-column:${columnId}`;
}

function columnIdFromSourceHandle(handleId: string | null): string | null {
  if (!handleId?.startsWith("source-column:")) {
    return null;
  }

  return handleId.slice("source-column:".length);
}

function columnIdFromTargetHandle(handleId: string | null): string | null {
  if (!handleId?.startsWith("target-column:")) {
    return null;
  }

  return handleId.slice("target-column:".length);
}

function cardinalityLabel(cardinality: DatabaseDiagramCardinality) {
  switch (cardinality) {
    case "one_to_one":
      return "1:1";
    case "one_to_many":
      return "1:N";
    case "many_to_one":
      return "N:1";
    case "many_to_many":
      return "N:N";
    default:
      return cardinality;
  }
}

type DiagramNodePositionChange = Extract<
  NodeChange<DiagramNode>,
  { type: "position" }
> & {
  position: { x: number; y: number };
};

function isNodePositionChange(
  change: NodeChange<DiagramNode>,
): change is DiagramNodePositionChange {
  return change.type === "position" && Boolean(change.position);
}

function InspectorPanel({
  copy,
  title,
  description,
  document,
  selected,
  onTitleChange,
  onDescriptionChange,
  onSelect,
  mutateDocument,
}: {
  copy: DesignCopy;
  title: string;
  description: string;
  document: DatabaseDiagramDocument;
  selected: SelectedElement;
  onTitleChange: (value: string) => void;
  onDescriptionChange: (value: string) => void;
  onSelect: (selected: SelectedElement) => void;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const table =
    selected.kind === "table"
      ? document.tables.find((item) => item.id === selected.id)
      : null;
  const relationship =
    selected.kind === "relationship"
      ? document.relationships.find((item) => item.id === selected.id)
      : null;
  const note =
    selected.kind === "note"
      ? document.notes.find((item) => item.id === selected.id)
      : null;
  const area =
    selected.kind === "area"
      ? document.areas.find((item) => item.id === selected.id)
      : null;
  const enumItem =
    selected.kind === "enum"
      ? document.enums.find((item) => item.id === selected.id)
      : null;

  return (
    <aside className="flex min-h-0 flex-col bg-card">
      <div className="border-b px-4 py-3">
        <div className="text-sm font-semibold">{copy.inspector}</div>
        <p className="mt-1 text-xs text-muted-foreground">{copy.noSelection}</p>
      </div>
      <div className="min-h-0 flex-1 px-4 py-4">
        {selected.kind === "diagram" ? (
          <DiagramInspector
            copy={copy}
            title={title}
            description={description}
            document={document}
            onTitleChange={onTitleChange}
            onDescriptionChange={onDescriptionChange}
            onSelect={onSelect}
          />
        ) : null}
        {table ? (
          <TableInspector
            copy={copy}
            table={table}
            onSelect={onSelect}
            mutateDocument={mutateDocument}
          />
        ) : null}
        {relationship ? (
          <RelationshipInspector
            copy={copy}
            relationship={relationship}
            document={document}
            mutateDocument={mutateDocument}
          />
        ) : null}
        {note ? (
          <NoteInspector
            copy={copy}
            note={note}
            mutateDocument={mutateDocument}
          />
        ) : null}
        {area ? (
          <AreaInspector
            copy={copy}
            area={area}
            mutateDocument={mutateDocument}
          />
        ) : null}
        {enumItem ? (
          <EnumInspector
            copy={copy}
            enumItem={enumItem}
            mutateDocument={mutateDocument}
          />
        ) : null}
      </div>
    </aside>
  );
}

function DiagramInspector({
  copy,
  title,
  description,
  document,
  onTitleChange,
  onDescriptionChange,
  onSelect,
}: {
  copy: DesignCopy;
  title: string;
  description: string;
  document: DatabaseDiagramDocument;
  onTitleChange: (value: string) => void;
  onDescriptionChange: (value: string) => void;
  onSelect: (selected: SelectedElement) => void;
}) {
  return (
    <Tabs defaultValue="overview" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-3">
        <TabsTrigger value="overview">{copy.overviewTab}</TabsTrigger>
        <TabsTrigger value="enums">{copy.enums}</TabsTrigger>
        <TabsTrigger value="document">{copy.documentTab}</TabsTrigger>
      </TabsList>
      <TabsContent value="overview" className={inspectorTabContentClassName}>
        <SectionTitle icon={FileJson} title={copy.diagram} />
        <Field label={copy.name}>
          <TextInput value={title} onChange={onTitleChange} />
        </Field>
        <Field label={copy.descriptionField}>
          <TextArea value={description} rows={3} onChange={onDescriptionChange} />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Metric label={copy.tables} value={document.tables.length} />
          <Metric label={copy.relationships} value={document.relationships.length} />
          <Metric label={copy.notes} value={document.notes.length} />
          <Metric label={copy.areas} value={document.areas.length} />
        </div>
      </TabsContent>
      <TabsContent value="enums" className={inspectorTabContentClassName}>
        <SectionTitle icon={Layers3} title={copy.enums} />
        {document.enums.length === 0 ? (
          <div className="rounded-md border bg-background p-3 text-xs text-muted-foreground">
            {copy.noSelection}
          </div>
        ) : (
          document.enums.map((enumItem) => (
            <button
              key={enumItem.id}
              type="button"
              className="flex w-full items-center justify-between rounded-md border bg-background px-3 py-2 text-left text-sm outline-none transition-colors hover:bg-muted/40 focus-visible:ring-[3px] focus-visible:ring-ring/50"
              onClick={() => onSelect({ kind: "enum", id: enumItem.id })}
            >
              <span className="truncate">{enumItem.name}</span>
              <Badge variant="outline" className="rounded-md">
                {enumItem.values.length}
              </Badge>
            </button>
          ))
        )}
      </TabsContent>
      <TabsContent value="document" className={inspectorTabContentClassName}>
        <SectionTitle icon={FileJson} title={copy.documentTab} />
        <div className="rounded-md border bg-background p-3 text-xs text-muted-foreground">
          {copy.updatedAt}: {document.version}
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Metric label={copy.relationships} value={document.relationships.length} />
          <Metric label={copy.enums} value={document.enums.length} />
        </div>
      </TabsContent>
    </Tabs>
  );
}

function TableInspector({
  copy,
  table,
  onSelect,
  mutateDocument,
}: {
  copy: DesignCopy;
  table: DatabaseDiagramTable;
  onSelect: (selected: SelectedElement) => void;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const updateTable = (patch: Partial<DatabaseDiagramTable>) => {
    mutateDocument((current) => {
      const oldTable = current.tables.find((item) => item.id === table.id);
      const nextName = patch.name ?? oldTable?.name;

      return {
        ...current,
        tables: current.tables.map((item) =>
          item.id === table.id ? { ...item, ...patch } : item,
        ),
        relationships:
          nextName && oldTable?.name !== nextName
            ? current.relationships.map((relationship) =>
                renameRelationshipTable(relationship, table.id, nextName),
              )
            : current.relationships,
      };
    });
  };

  const updateColumn = (
    columnId: string,
    patch: Partial<DatabaseDiagramColumn>,
  ) => {
    mutateDocument((current) => {
      const oldColumn = table.columns.find((column) => column.id === columnId);
      const nextName = patch.name ?? oldColumn?.name;

      return {
        ...current,
        tables: current.tables.map((item) =>
          item.id === table.id
            ? {
                ...item,
                columns: item.columns.map((column) =>
                  column.id === columnId ? { ...column, ...patch } : column,
                ),
                indexes:
                  nextName && oldColumn?.name !== nextName
                    ? item.indexes.map((index) => ({
                        ...index,
                        columns: index.columns.map((column) =>
                          column === oldColumn?.name ? nextName : column,
                        ),
                      }))
                    : item.indexes,
              }
            : item,
        ),
        relationships:
          nextName && oldColumn?.name !== nextName
            ? current.relationships.map((relationship) =>
                renameRelationshipColumn(relationship, table.id, columnId, nextName),
              )
            : current.relationships,
      };
    });
  };

  const addColumn = () => {
    const column: DatabaseDiagramColumn = {
      id: createId("column"),
      name: `column_${table.columns.length + 1}`,
      data_type: "text",
      nullable: true,
      primary_key: false,
      unique: false,
      default_value: undefined,
      comment: undefined,
    };

    updateTable({ columns: [...table.columns, column] });
  };

  const deleteColumn = (columnId: string) => {
    mutateDocument((current) => ({
      ...current,
      tables: current.tables.map((item) =>
        item.id === table.id
          ? {
              ...item,
              columns: item.columns.filter((column) => column.id !== columnId),
            }
          : item,
      ),
      relationships: current.relationships.filter(
        (relationship) =>
          !(
            (relationship.source.table_id === table.id &&
              relationship.source.column_id === columnId) ||
            (relationship.target.table_id === table.id &&
              relationship.target.column_id === columnId)
          ),
      ),
    }));
  };

  const addIndex = () => {
    const firstColumn = table.columns[0]?.name ?? "id";
    const index: DatabaseDiagramIndex = {
      id: createId("index"),
      name: `${table.name}_${firstColumn}_idx`,
      columns: [firstColumn],
      unique: false,
      method: "btree",
    };

    updateTable({ indexes: [...table.indexes, index] });
  };

  const updateIndex = (indexId: string, patch: Partial<DatabaseDiagramIndex>) => {
    updateTable({
      indexes: table.indexes.map((index) =>
        index.id === indexId ? { ...index, ...patch } : index,
      ),
    });
  };

  const deleteTable = () => {
    mutateDocument((current) => ({
      ...current,
      tables: current.tables.filter((item) => item.id !== table.id),
      relationships: current.relationships.filter(
        (relationship) =>
          relationship.source.table_id !== table.id &&
          relationship.target.table_id !== table.id,
      ),
    }));
    onSelect({ kind: "diagram" });
  };

  return (
    <Tabs defaultValue="basic" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-4">
        <TabsTrigger value="basic">{copy.basicTab}</TabsTrigger>
        <TabsTrigger value="fields">{copy.fieldsTab}</TabsTrigger>
        <TabsTrigger value="indexes">{copy.indexesTab}</TabsTrigger>
        <TabsTrigger value="style">{copy.styleTab}</TabsTrigger>
      </TabsList>
      <TabsContent value="basic" className={inspectorTabContentClassName}>
        <SectionTitle icon={Table2} title={copy.table} />
        <Field label={copy.name}>
          <TextInput value={table.name} onChange={(name) => updateTable({ name })} />
        </Field>
        <Field label={copy.schema}>
          <TextInput
            value={table.schema ?? ""}
            onChange={(schema) => updateTable({ schema })}
          />
        </Field>
        <Field label={copy.comment}>
          <TextArea
            value={table.comment ?? ""}
            rows={4}
            onChange={(comment) => updateTable({ comment })}
          />
        </Field>
      </TabsContent>
      <TabsContent value="fields" className={inspectorTabContentClassName}>
        <div className="flex items-center justify-between gap-2">
          <SectionTitle icon={Rows3} title={copy.columns} compact />
          <Button type="button" variant="outline" size="sm" onClick={addColumn}>
            <Plus className="size-4" aria-hidden />
            {copy.addColumn}
          </Button>
        </div>
        {table.columns.map((column) => (
          <div key={column.id} className="space-y-2 rounded-lg border bg-background p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0 truncate text-xs font-semibold">
                {column.name}
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={copy.delete}
                title={copy.delete}
                onClick={() => deleteColumn(column.id)}
              >
                <Trash2 className="size-4" aria-hidden />
              </Button>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <Field label={copy.name}>
                <TextInput
                  value={column.name}
                  onChange={(name) => updateColumn(column.id, { name })}
                />
              </Field>
              <Field label={copy.dataType}>
                <TextInput
                  value={column.data_type}
                  onChange={(data_type) =>
                    updateColumn(column.id, { data_type })
                  }
                />
              </Field>
            </div>
            <div className="grid grid-cols-2 gap-2">
              <CheckField
                label={copy.nullable}
                checked={column.nullable}
                onChange={(nullable) => updateColumn(column.id, { nullable })}
              />
              <CheckField
                label={copy.primaryKey}
                checked={column.primary_key}
                onChange={(primary_key) =>
                  updateColumn(column.id, {
                    primary_key,
                    nullable: primary_key ? false : column.nullable,
                  })
                }
              />
              <CheckField
                label={copy.unique}
                checked={column.unique}
                onChange={(unique) => updateColumn(column.id, { unique })}
              />
            </div>
            <Field label={copy.defaultValue}>
              <TextInput
                value={column.default_value ?? ""}
                onChange={(default_value) =>
                  updateColumn(column.id, { default_value })
                }
              />
            </Field>
            <Field label={copy.comment}>
              <TextArea
                value={column.comment ?? ""}
                rows={2}
                onChange={(comment) => updateColumn(column.id, { comment })}
              />
            </Field>
          </div>
        ))}
      </TabsContent>
      <TabsContent value="indexes" className={inspectorTabContentClassName}>
        <div className="flex items-center justify-between gap-2">
          <SectionTitle icon={FileJson} title={copy.indexes} compact />
          <Button type="button" variant="outline" size="sm" onClick={addIndex}>
            <Plus className="size-4" aria-hidden />
            {copy.addIndex}
          </Button>
        </div>
        {table.indexes.map((index) => (
          <div key={index.id} className="space-y-2 rounded-lg border bg-background p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0 truncate text-xs font-semibold">
                {index.name}
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={copy.delete}
                title={copy.delete}
                onClick={() =>
                  updateTable({
                    indexes: table.indexes.filter((item) => item.id !== index.id),
                  })
                }
              >
                <Trash2 className="size-4" aria-hidden />
              </Button>
            </div>
            <Field label={copy.name}>
              <TextInput
                value={index.name}
                onChange={(name) => updateIndex(index.id, { name })}
              />
            </Field>
            <Field label={copy.columns}>
              <TextInput
                value={index.columns.join(", ")}
                onChange={(value) =>
                  updateIndex(index.id, {
                    columns: value
                      .split(",")
                      .map((item) => item.trim())
                      .filter(Boolean),
                  })
                }
              />
            </Field>
            <CheckField
              label={copy.unique}
              checked={index.unique}
              onChange={(unique) => updateIndex(index.id, { unique })}
            />
          </div>
        ))}
      </TabsContent>
      <TabsContent value="style" className={inspectorTabContentClassName}>
        <SectionTitle icon={BoxSelect} title={copy.styleTab} />
        <Field label={copy.color}>
          <ColorInput
            value={table.color ?? "#2563eb"}
            onChange={(color) => updateTable({ color })}
          />
        </Field>
        <div className="grid grid-cols-2 gap-2">
          <Field label="X">
            <NumberInput
              value={table.position.x}
              onChange={(x) => updateTable({ position: { ...table.position, x } })}
            />
          </Field>
          <Field label="Y">
            <NumberInput
              value={table.position.y}
              onChange={(y) => updateTable({ position: { ...table.position, y } })}
            />
          </Field>
        </div>
        <Button type="button" variant="destructive" onClick={deleteTable}>
          <Trash2 className="size-4" aria-hidden />
          {copy.delete}
        </Button>
      </TabsContent>
    </Tabs>
  );
}

function RelationshipInspector({
  copy,
  relationship,
  document,
  mutateDocument,
}: {
  copy: DesignCopy;
  relationship: DatabaseDiagramRelationship;
  document: DatabaseDiagramDocument;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const updateRelationship = (patch: Partial<DatabaseDiagramRelationship>) => {
    mutateDocument((current) => ({
      ...current,
      relationships: current.relationships.map((item) =>
        item.id === relationship.id ? { ...item, ...patch } : item,
      ),
    }));
  };

  const deleteRelationship = () => {
    mutateDocument((current) => ({
      ...current,
      relationships: current.relationships.filter(
        (item) => item.id !== relationship.id,
      ),
    }));
  };

  return (
    <Tabs defaultValue="basic" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-3">
        <TabsTrigger value="basic">{copy.basicTab}</TabsTrigger>
        <TabsTrigger value="endpoints">{copy.endpointsTab}</TabsTrigger>
        <TabsTrigger value="rules">{copy.rulesTab}</TabsTrigger>
      </TabsList>
      <TabsContent value="basic" className={inspectorTabContentClassName}>
        <SectionTitle icon={GitBranch} title={copy.relationship} />
        <Field label={copy.name}>
          <TextInput
            value={relationship.name}
            onChange={(name) => updateRelationship({ name })}
          />
        </Field>
        <Field label={copy.cardinality}>
          <select
            value={relationship.cardinality}
            className={inputClassName}
            onChange={(event) =>
              updateRelationship({
                cardinality: event.target.value as DatabaseDiagramCardinality,
              })
            }
          >
            {cardinalityOptions.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </Field>
        <Button type="button" variant="destructive" onClick={deleteRelationship}>
          <Trash2 className="size-4" aria-hidden />
          {copy.delete}
        </Button>
      </TabsContent>
      <TabsContent value="endpoints" className={inspectorTabContentClassName}>
        <EndpointField
          label={copy.source}
          endpoint={relationship.source}
          tables={document.tables}
          onChange={(source) => updateRelationship({ source })}
        />
        <EndpointField
          label={copy.target}
          endpoint={relationship.target}
          tables={document.tables}
          onChange={(target) => updateRelationship({ target })}
        />
      </TabsContent>
      <TabsContent value="rules" className={inspectorTabContentClassName}>
        <SectionTitle icon={GitBranch} title={copy.rulesTab} />
        <Field label={copy.onUpdate}>
          <TextInput
            value={relationship.on_update ?? ""}
            onChange={(on_update) => updateRelationship({ on_update })}
          />
        </Field>
        <Field label={copy.onDelete}>
          <TextInput
            value={relationship.on_delete ?? ""}
            onChange={(on_delete) => updateRelationship({ on_delete })}
          />
        </Field>
      </TabsContent>
    </Tabs>
  );
}

function EndpointField({
  label,
  endpoint,
  tables,
  onChange,
}: {
  label: string;
  endpoint: DatabaseDiagramRelationshipEndpoint;
  tables: DatabaseDiagramTable[];
  onChange: (endpoint: DatabaseDiagramRelationshipEndpoint) => void;
}) {
  const selectedTable = tables.find((table) => table.id === endpoint.table_id);

  return (
    <div className="space-y-2">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <select
        value={endpoint.table_id}
        className={inputClassName}
        onChange={(event) => {
          const table = tables.find((item) => item.id === event.target.value);
          const column = table?.columns[0];

          if (table && column) {
            onChange(endpointFor(table, column));
          }
        }}
      >
        {tables.map((table) => (
          <option key={table.id} value={table.id}>
            {table.name}
          </option>
        ))}
      </select>
      <select
        value={endpoint.column_id}
        className={inputClassName}
        onChange={(event) => {
          const column = selectedTable?.columns.find(
            (item) => item.id === event.target.value,
          );

          if (selectedTable && column) {
            onChange(endpointFor(selectedTable, column));
          }
        }}
      >
        {(selectedTable?.columns ?? []).map((column) => (
          <option key={column.id} value={column.id}>
            {column.name}
          </option>
        ))}
      </select>
    </div>
  );
}

function NoteInspector({
  copy,
  note,
  mutateDocument,
}: {
  copy: DesignCopy;
  note: DatabaseDiagramNote;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const updateNote = (patch: Partial<DatabaseDiagramNote>) => {
    mutateDocument((current) => ({
      ...current,
      notes: current.notes.map((item) =>
        item.id === note.id ? { ...item, ...patch } : item,
      ),
    }));
  };

  return (
    <Tabs defaultValue="basic" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-2">
        <TabsTrigger value="basic">{copy.basicTab}</TabsTrigger>
        <TabsTrigger value="style">{copy.styleTab}</TabsTrigger>
      </TabsList>
      <TabsContent value="basic" className={inspectorTabContentClassName}>
        <SectionTitle icon={StickyNote} title={copy.note} />
        <Field label={copy.name}>
          <TextInput
            value={note.title}
            onChange={(title) => updateNote({ title })}
          />
        </Field>
        <Field label={copy.body}>
          <TextArea
            value={note.body}
            rows={8}
            onChange={(body) => updateNote({ body })}
          />
        </Field>
      </TabsContent>
      <TabsContent value="style" className={inspectorTabContentClassName}>
        <SectionTitle icon={BoxSelect} title={copy.position} />
        <div className="grid grid-cols-2 gap-2">
          <Field label="X">
            <NumberInput
              value={note.position.x}
              onChange={(x) => updateNote({ position: { ...note.position, x } })}
            />
          </Field>
          <Field label="Y">
            <NumberInput
              value={note.position.y}
              onChange={(y) => updateNote({ position: { ...note.position, y } })}
            />
          </Field>
        </div>
        <Button
          type="button"
          variant="destructive"
          onClick={() =>
            mutateDocument((current) => ({
              ...current,
              notes: current.notes.filter((item) => item.id !== note.id),
            }))
          }
        >
          <Trash2 className="size-4" aria-hidden />
          {copy.delete}
        </Button>
      </TabsContent>
    </Tabs>
  );
}

function AreaInspector({
  copy,
  area,
  mutateDocument,
}: {
  copy: DesignCopy;
  area: DatabaseDiagramArea;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const updateArea = (patch: Partial<DatabaseDiagramArea>) => {
    mutateDocument((current) => ({
      ...current,
      areas: current.areas.map((item) =>
        item.id === area.id ? { ...item, ...patch } : item,
      ),
    }));
  };

  return (
    <Tabs defaultValue="basic" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-2">
        <TabsTrigger value="basic">{copy.basicTab}</TabsTrigger>
        <TabsTrigger value="style">{copy.styleTab}</TabsTrigger>
      </TabsList>
      <TabsContent value="basic" className={inspectorTabContentClassName}>
        <SectionTitle icon={BoxSelect} title={copy.area} />
        <Field label={copy.name}>
          <TextInput
            value={area.title}
            onChange={(title) => updateArea({ title })}
          />
        </Field>
        <Field label={copy.color}>
          <ColorInput
            value={area.color ?? "#dbeafe"}
            onChange={(color) => updateArea({ color })}
          />
        </Field>
      </TabsContent>
      <TabsContent value="style" className={inspectorTabContentClassName}>
        <SectionTitle icon={BoxSelect} title={copy.size} />
        <div className="grid grid-cols-2 gap-2">
          <Field label="X">
            <NumberInput
              value={area.position.x}
              onChange={(x) => updateArea({ position: { ...area.position, x } })}
            />
          </Field>
          <Field label="Y">
            <NumberInput
              value={area.position.y}
              onChange={(y) => updateArea({ position: { ...area.position, y } })}
            />
          </Field>
          <Field label={copy.width}>
            <NumberInput
              value={area.size.width}
              min={120}
              onChange={(width) => updateArea({ size: { ...area.size, width } })}
            />
          </Field>
          <Field label={copy.height}>
            <NumberInput
              value={area.size.height}
              min={80}
              onChange={(height) => updateArea({ size: { ...area.size, height } })}
            />
          </Field>
        </div>
        <Button
          type="button"
          variant="destructive"
          onClick={() =>
            mutateDocument((current) => ({
              ...current,
              areas: current.areas.filter((item) => item.id !== area.id),
            }))
          }
        >
          <Trash2 className="size-4" aria-hidden />
          {copy.delete}
        </Button>
      </TabsContent>
    </Tabs>
  );
}

function EnumInspector({
  copy,
  enumItem,
  mutateDocument,
}: {
  copy: DesignCopy;
  enumItem: DatabaseDiagramEnum;
  mutateDocument: (
    updater: (current: DatabaseDiagramDocument) => DatabaseDiagramDocument,
  ) => void;
}) {
  const updateEnum = (patch: Partial<DatabaseDiagramEnum>) => {
    mutateDocument((current) => ({
      ...current,
      enums: current.enums.map((item) =>
        item.id === enumItem.id ? { ...item, ...patch } : item,
      ),
    }));
  };

  return (
    <Tabs defaultValue="basic" className={inspectorTabsClassName}>
      <TabsList className="grid w-full grid-cols-2">
        <TabsTrigger value="basic">{copy.basicTab}</TabsTrigger>
        <TabsTrigger value="values">{copy.values}</TabsTrigger>
      </TabsList>
      <TabsContent value="basic" className={inspectorTabContentClassName}>
        <SectionTitle icon={Layers3} title={copy.enum} />
        <Field label={copy.name}>
          <TextInput
            value={enumItem.name}
            onChange={(name) => updateEnum({ name })}
          />
        </Field>
        <Button
          type="button"
          variant="destructive"
          onClick={() =>
            mutateDocument((current) => ({
              ...current,
              enums: current.enums.filter((item) => item.id !== enumItem.id),
            }))
          }
        >
          <Trash2 className="size-4" aria-hidden />
          {copy.delete}
        </Button>
      </TabsContent>
      <TabsContent value="values" className={inspectorTabContentClassName}>
        <div className="flex items-center justify-between gap-2">
          <SectionTitle icon={Rows3} title={copy.values} compact />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              updateEnum({
                values: [
                  ...enumItem.values,
                  {
                    id: createId("enum_value"),
                    name: `value_${enumItem.values.length + 1}`,
                    comment: undefined,
                  },
                ],
              })
            }
          >
            <Plus className="size-4" aria-hidden />
            {copy.addValue}
          </Button>
        </div>
        {enumItem.values.map((value) => (
          <div key={value.id} className="space-y-2 rounded-lg border bg-background p-3">
            <div className="flex items-center gap-2">
              <TextInput
                value={value.name}
                onChange={(name) =>
                  updateEnum({
                    values: enumItem.values.map((item) =>
                      item.id === value.id ? { ...item, name } : item,
                    ),
                  })
                }
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label={copy.delete}
                title={copy.delete}
                onClick={() =>
                  updateEnum({
                    values: enumItem.values.filter((item) => item.id !== value.id),
                  })
                }
              >
                <Trash2 className="size-4" aria-hidden />
              </Button>
            </div>
            <TextInput
              value={value.comment ?? ""}
              onChange={(comment) =>
                updateEnum({
                  values: enumItem.values.map((item) =>
                    item.id === value.id ? { ...item, comment } : item,
                  ),
                })
              }
            />
          </div>
        ))}
      </TabsContent>
    </Tabs>
  );
}

function SectionTitle({
  icon: Icon,
  title,
  compact = false,
}: {
  icon: LucideIcon;
  title: string;
  compact?: boolean;
}) {
  return (
    <div className={cn("flex items-center gap-2", !compact && "pb-1")}>
      <span className="flex size-7 items-center justify-center rounded-md border bg-background">
        <Icon className="size-4 text-muted-foreground" aria-hidden />
      </span>
      <div className="text-sm font-semibold">{title}</div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
    </label>
  );
}

function CheckField({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex h-9 items-center gap-2 rounded-md border bg-background px-2 text-xs">
      <input
        type="checkbox"
        checked={checked}
        className="size-4 accent-primary"
        onChange={(event) => onChange(event.target.checked)}
      />
      {label}
    </label>
  );
}

const inputClassName =
  "h-9 w-full rounded-md border bg-background px-2 text-sm outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50";
const inspectorTabsClassName = "flex h-full min-h-0 flex-col";
const inspectorTabContentClassName =
  "min-h-0 flex-1 space-y-4 overflow-y-auto pr-1";

function TextInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <input
      value={value}
      className={inputClassName}
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function TextArea({
  value,
  rows,
  onChange,
}: {
  value: string;
  rows: number;
  onChange: (value: string) => void;
}) {
  return (
    <textarea
      value={value}
      rows={rows}
      className="w-full resize-y rounded-md border bg-background px-2 py-2 text-sm outline-none transition-shadow focus-visible:ring-[3px] focus-visible:ring-ring/50"
      onChange={(event) => onChange(event.target.value)}
    />
  );
}

function NumberInput({
  value,
  min,
  onChange,
}: {
  value: number;
  min?: number;
  onChange: (value: number) => void;
}) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      className={inputClassName}
      onChange={(event) => {
        const parsed = Number.parseInt(event.target.value, 10);

        if (Number.isFinite(parsed)) {
          onChange(min === undefined ? parsed : Math.max(min, parsed));
        }
      }}
    />
  );
}

function ColorInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="flex gap-2">
      <input
        type="color"
        value={safeColor(value)}
        className="h-9 w-12 rounded-md border bg-background p-1"
        onChange={(event) => onChange(event.target.value)}
      />
      <TextInput value={value} onChange={onChange} />
    </div>
  );
}

function createStarterDocument(): DatabaseDiagramDocument {
  const tableId = createId("table");
  const columnId = createId("column");

  return {
    version: 1,
    database_engine: "postgres",
    tables: [
      {
        id: tableId,
        name: "main_table",
        schema: "public",
        position: { x: 180, y: 180 },
        color: "#2563eb",
        comment: undefined,
        columns: [
          {
            id: columnId,
            name: "id",
            data_type: "uuid",
            nullable: false,
            primary_key: true,
            unique: true,
            default_value: undefined,
            comment: undefined,
          },
        ],
        indexes: [],
      },
    ],
    relationships: [],
    notes: [],
    areas: [],
    enums: [],
  };
}

function normalizeDocument(
  document: Partial<DatabaseDiagramDocument>,
): DatabaseDiagramDocument {
  return {
    version: document.version ?? 1,
    database_engine: document.database_engine ?? "postgres",
    tables: document.tables ?? [],
    relationships: document.relationships ?? [],
    notes: document.notes ?? [],
    areas: document.areas ?? [],
    enums: document.enums ?? [],
  };
}

function prepareDocumentForSave(
  document: DatabaseDiagramDocument,
): DatabaseDiagramDocument {
  return {
    ...document,
    tables: document.tables.map((table) => ({
      ...table,
      schema: optionalText(table.schema),
      color: optionalText(table.color),
      comment: optionalText(table.comment),
      columns: table.columns.map((column) => ({
        ...column,
        default_value: optionalText(column.default_value),
        comment: optionalText(column.comment),
      })),
      indexes: table.indexes.map((index) => ({
        ...index,
        method: optionalText(index.method),
      })),
    })),
    relationships: document.relationships.map((relationship) => ({
      ...relationship,
      on_update: optionalText(relationship.on_update),
      on_delete: optionalText(relationship.on_delete),
    })),
    areas: document.areas.map((area) => ({
      ...area,
      color: optionalText(area.color),
    })),
    enums: document.enums.map((enumItem) => ({
      ...enumItem,
      values: enumItem.values.map((value) => ({
        ...value,
        comment: optionalText(value.comment),
      })),
    })),
  };
}

function optionalText(value: string | undefined): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}

function endpointFor(
  table: DatabaseDiagramTable,
  column: DatabaseDiagramColumn,
): DatabaseDiagramRelationshipEndpoint {
  return {
    table_id: table.id,
    table_name: table.name,
    column_id: column.id,
    column_name: column.name,
  };
}

function renameRelationshipTable(
  relationship: DatabaseDiagramRelationship,
  tableId: string,
  name: string,
): DatabaseDiagramRelationship {
  return {
    ...relationship,
    source:
      relationship.source.table_id === tableId
        ? { ...relationship.source, table_name: name }
        : relationship.source,
    target:
      relationship.target.table_id === tableId
        ? { ...relationship.target, table_name: name }
        : relationship.target,
  };
}

function renameRelationshipColumn(
  relationship: DatabaseDiagramRelationship,
  tableId: string,
  columnId: string,
  name: string,
): DatabaseDiagramRelationship {
  return {
    ...relationship,
    source:
      relationship.source.table_id === tableId &&
      relationship.source.column_id === columnId
        ? { ...relationship.source, column_name: name }
        : relationship.source,
    target:
      relationship.target.table_id === tableId &&
      relationship.target.column_id === columnId
        ? { ...relationship.target, column_name: name }
        : relationship.target,
  };
}

function createId(prefix: string): string {
  const random =
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}_${Math.random().toString(36).slice(2)}`;

  return `${prefix}_${random.replaceAll("-", "_")}`;
}

function safeColor(value: string): string {
  return /^#[0-9a-fA-F]{6}$/.test(value) ? value : "#2563eb";
}

function slugify(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "database-diagram"
  );
}

function formatDateTime(value: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
