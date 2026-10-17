import type { RadioCatalogOption, RadioSearchResult } from "@repo/api-client";
import { usePlayback } from "@repo/ui";
import {
	useInfiniteQuery,
	useMutation,
	useQueryClient,
} from "@tanstack/react-query";
import {
	AudioLines,
	BadgeCheck,
	Check,
	ChevronDown,
	Grid2X2,
	Headphones,
	Info,
	List,
	MapPin,
	Radio,
	Search,
	ShieldAlert,
	ShieldQuestion,
	SlidersHorizontal,
	X,
} from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	COLLECTION_PAGE_CONTAINER_CLASS,
	CollectionPageContainer,
	CARD_INTERACTION_CLASS,
} from "#/components/collection-grid-layout";
import { Button } from "#/components/ui/button";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuTrigger,
} from "#/components/ui/context-menu";
import { Input } from "#/components/ui/input";
import { apiClient } from "#/lib/api";
import {
	HEADER_SEARCH_CONTAINER_CLASS,
	HEADER_SEARCH_INPUT_CLASS,
} from "#/lib/page-layout-classes";
import { cn } from "#/lib/utils";

const PAGE_SIZE = 40;
const VISIBLE_REFRESH_MS = 30000;
const ALL_FILTER_VALUE = "all";
const MIN_ARTWORK_IMAGE_SIZE = 48;
const MAX_ARTWORK_ASPECT_RATIO = 3;
const FILTER_TRIGGER_CLASS =
	"h-11 rounded-lg border border-input bg-player text-player-foreground shadow-xs";
const DEFAULT_CATALOG_FILTERS: CatalogFilters = {
	q: "",
	country: ALL_FILTER_VALUE,
	tag: ALL_FILTER_VALUE,
	codec: ALL_FILTER_VALUE,
	minBitrate: ALL_FILTER_VALUE,
};
const SKELETON_CARD_KEYS = [
	"skeleton-1",
	"skeleton-2",
	"skeleton-3",
	"skeleton-4",
	"skeleton-5",
	"skeleton-6",
	"skeleton-7",
	"skeleton-8",
];

const discoverQueryKeys = {
	catalog: (filters: CatalogFilters) => ["radio", "discover", filters] as const,
};

const ISO_COUNTRY_CODES = [
	"AD",
	"AE",
	"AF",
	"AG",
	"AI",
	"AL",
	"AM",
	"AO",
	"AQ",
	"AR",
	"AS",
	"AT",
	"AU",
	"AW",
	"AX",
	"AZ",
	"BA",
	"BB",
	"BD",
	"BE",
	"BF",
	"BG",
	"BH",
	"BI",
	"BJ",
	"BL",
	"BM",
	"BN",
	"BO",
	"BQ",
	"BR",
	"BS",
	"BT",
	"BV",
	"BW",
	"BY",
	"BZ",
	"CA",
	"CC",
	"CD",
	"CF",
	"CG",
	"CH",
	"CI",
	"CK",
	"CL",
	"CM",
	"CN",
	"CO",
	"CR",
	"CU",
	"CV",
	"CW",
	"CX",
	"CY",
	"CZ",
	"DE",
	"DJ",
	"DK",
	"DM",
	"DO",
	"DZ",
	"EC",
	"EE",
	"EG",
	"EH",
	"ER",
	"ES",
	"ET",
	"FI",
	"FJ",
	"FK",
	"FM",
	"FO",
	"FR",
	"GA",
	"GB",
	"GD",
	"GE",
	"GF",
	"GG",
	"GH",
	"GI",
	"GL",
	"GM",
	"GN",
	"GP",
	"GQ",
	"GR",
	"GS",
	"GT",
	"GU",
	"GW",
	"GY",
	"HK",
	"HM",
	"HN",
	"HR",
	"HT",
	"HU",
	"ID",
	"IE",
	"IL",
	"IM",
	"IN",
	"IO",
	"IQ",
	"IR",
	"IS",
	"IT",
	"JE",
	"JM",
	"JO",
	"JP",
	"KE",
	"KG",
	"KH",
	"KI",
	"KM",
	"KN",
	"KP",
	"KR",
	"KW",
	"KY",
	"KZ",
	"LA",
	"LB",
	"LC",
	"LI",
	"LK",
	"LR",
	"LS",
	"LT",
	"LU",
	"LV",
	"LY",
	"MA",
	"MC",
	"MD",
	"ME",
	"MF",
	"MG",
	"MH",
	"MK",
	"ML",
	"MM",
	"MN",
	"MO",
	"MP",
	"MQ",
	"MR",
	"MS",
	"MT",
	"MU",
	"MV",
	"MW",
	"MX",
	"MY",
	"MZ",
	"NA",
	"NC",
	"NE",
	"NF",
	"NG",
	"NI",
	"NL",
	"NO",
	"NP",
	"NR",
	"NU",
	"NZ",
	"OM",
	"PA",
	"PE",
	"PF",
	"PG",
	"PH",
	"PK",
	"PL",
	"PM",
	"PN",
	"PR",
	"PS",
	"PT",
	"PW",
	"PY",
	"QA",
	"RE",
	"RO",
	"RS",
	"RU",
	"RW",
	"SA",
	"SB",
	"SC",
	"SD",
	"SE",
	"SG",
	"SH",
	"SI",
	"SJ",
	"SK",
	"SL",
	"SM",
	"SN",
	"SO",
	"SR",
	"SS",
	"ST",
	"SV",
	"SX",
	"SY",
	"SZ",
	"TC",
	"TD",
	"TF",
	"TG",
	"TH",
	"TJ",
	"TK",
	"TL",
	"TM",
	"TN",
	"TO",
	"TR",
	"TT",
	"TV",
	"TW",
	"TZ",
	"UA",
	"UG",
	"UM",
	"US",
	"UY",
	"UZ",
	"VA",
	"VC",
	"VE",
	"VG",
	"VI",
	"VN",
	"VU",
	"WF",
	"WS",
	"YE",
	"YT",
	"ZA",
	"ZM",
	"ZW",
];

const GENRE_OPTIONS = [
	"rock",
	"pop",
	"jazz",
	"folk",
	"classical",
	"blues",
	"electronic",
	"dance",
	"house",
	"techno",
	"trance",
	"ambient",
	"lounge",
	"hip hop",
	"r&b",
	"soul",
	"funk",
	"reggae",
	"metal",
	"punk",
	"country",
	"latin",
	"world",
	"news",
	"talk",
	"sports",
	"public radio",
	"chillout",
	"psychedelic",
	"progressive",
	"oldies",
	"disco",
	"alternative",
	"indie",
];

type CatalogFilters = {
	q: string;
	country: string;
	tag: string;
	codec: string;
	minBitrate: string;
};

type ViewMode = "grid" | "list";

export function RadioDiscoverPage() {
	const queryClient = useQueryClient();
	const { playRadioCatalogPreview } = usePlayback();
	const [filters, setFilters] = useState<CatalogFilters>(
		DEFAULT_CATALOG_FILTERS,
	);
	const [viewMode, setViewMode] = useState<ViewMode>("grid");
	const [areFiltersOpen, setAreFiltersOpen] = useState(false);
	const [selectedEntry, setSelectedEntry] = useState<RadioSearchResult | null>(
		null,
	);
	const [previewErrorUuid, setPreviewErrorUuid] = useState<string | null>(null);
	const [visibleStationIds, setVisibleStationIds] = useState<Set<string>>(
		() => new Set(),
	);
	const loadMoreRef = useRef<HTMLDivElement | null>(null);

	const catalog = useInfiniteQuery({
		queryKey: discoverQueryKeys.catalog(filters),
		initialPageParam: 0,
		queryFn: ({ pageParam }) =>
			apiClient.searchRadioStations({
				q: filters.q.trim() || undefined,
				country: selectedFilterValue(filters.country),
				tag: selectedFilterValue(filters.tag),
				codec:
					filters.codec !== ALL_FILTER_VALUE && filters.codec !== "AAC"
						? filters.codec
						: undefined,
				codecGroup: filters.codec === "AAC" ? "aac" : undefined,
				minBitrate:
					filters.minBitrate !== ALL_FILTER_VALUE
						? Number(filters.minBitrate)
						: undefined,
				limit: PAGE_SIZE,
				offset: pageParam,
			}),
		getNextPageParam: (lastPage, allPages) =>
			lastPage.items.length < PAGE_SIZE
				? undefined
				: allPages.length * PAGE_SIZE,
		refetchInterval: visibleStationIds.size > 0 ? VISIBLE_REFRESH_MS : false,
	});

	const importStation = useMutation({
		mutationFn: (result: RadioSearchResult) =>
			apiClient.importRadioStation({ result }),
		onSuccess: async () => {
			await queryClient.invalidateQueries({ queryKey: ["radio", "stations"] });
		},
	});

	const entries = useMemo(
		() => catalog.data?.pages.flatMap((page) => page.items) ?? [],
		[catalog.data],
	);
	const countryOptions = useMemo(() => buildCountryOptions(), []);
	const tagOptions = useMemo(() => GENRE_OPTIONS.map((name) => ({ name })), []);

	const handleVisibilityChange = useCallback(
		(stationUuid: string, isVisible: boolean) => {
			setVisibleStationIds((current) => {
				const next = new Set(current);
				if (isVisible) {
					next.add(stationUuid);
				} else {
					next.delete(stationUuid);
				}
				return next;
			});
		},
		[],
	);

	const handlePreview = useCallback(
		async (entry: RadioSearchResult) => {
			setPreviewErrorUuid(null);
			try {
				await playRadioCatalogPreview(entry);
			} catch {
				setPreviewErrorUuid(entry.stationUuid);
			}
		},
		[playRadioCatalogPreview],
	);

	const { hasNextPage, isFetchingNextPage, fetchNextPage } = catalog;
	useEffect(() => {
		const node = loadMoreRef.current;
		if (!node || typeof IntersectionObserver === "undefined") return;
		const observer = new IntersectionObserver(([record]) => {
			if (record.isIntersecting && hasNextPage && !isFetchingNextPage) {
				void fetchNextPage();
			}
		});
		observer.observe(node);
		return () => observer.disconnect();
	}, [hasNextPage, isFetchingNextPage, fetchNextPage]);

	return (
		<div className="flex min-h-0 flex-1 flex-col overflow-hidden">
			<header className="sticky top-0 z-40 shrink-0 bg-background/80 px-(--shell-inset) py-3 backdrop-blur">
				<div
					className={`flex flex-col gap-2 ${COLLECTION_PAGE_CONTAINER_CLASS}`}
				>
					<div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
						<div className="min-w-0">
							<h1 className="font-semibold text-2xl text-heading tracking-normal">
								Discover Radio
							</h1>
						</div>

						<div className="flex min-w-0 flex-1 items-center gap-2 xl:max-w-2xl xl:justify-end">
							<label
								className={HEADER_SEARCH_CONTAINER_CLASS}
								htmlFor="radio-discover-search"
							>
								<Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-caption" />
								<Input
									id="radio-discover-search"
									className={`${HEADER_SEARCH_INPUT_CLASS} text-player-foreground placeholder:text-player-foreground/55`}
									value={filters.q}
									onChange={(event) =>
										setFilters((current) => ({
											...current,
											q: event.target.value,
										}))
									}
									placeholder="Search station name..."
									type="search"
								/>
							</label>
							<Button
								type="button"
								variant="ghost"
								size="icon"
								aria-label="Filters"
								className="size-10 text-caption hover:text-heading"
								onClick={() => setAreFiltersOpen(true)}
							>
								<SlidersHorizontal className="size-5" />
							</Button>
						</div>
					</div>

					<div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
						<p className="max-w-2xl text-foreground text-xs">
							Browse Radio Browser catalog entries A-Z.
						</p>
					</div>
				</div>
			</header>

			<div className="min-h-0 flex-1 overflow-y-auto px-(--shell-inset) py-6 scrollbar-hidden">
				<CollectionPageContainer>
					{catalog.isLoading ? (
						<div className="grid grid-cols-panels gap-4">
							{SKELETON_CARD_KEYS.map((key) => (
								<SkeletonCard key={key} />
							))}
						</div>
					) : null}

					{catalog.isError ? (
						<div className="rounded-md border border-destructive/40 p-6 text-destructive text-sm">
							Failed to load Radio Browser catalog.
						</div>
					) : null}

					{!catalog.isLoading && entries.length === 0 ? (
						<div className="rounded-md border border-dashed border-border p-6 text-caption text-sm">
							No catalog entries match these filters.
						</div>
					) : null}

					{viewMode === "grid" ? (
						<div
							data-testid="radio-catalog-grid"
							className="grid grid-cols-panels gap-3"
						>
							{entries.map((entry) => (
								<CatalogCard
									key={entry.stationUuid}
									entry={entry}
									isImporting={importStation.isPending}
									hasPreviewError={previewErrorUuid === entry.stationUuid}
									onImport={() => importStation.mutate(entry)}
									onPreview={() => void handlePreview(entry)}
									onDetails={() => setSelectedEntry(entry)}
									onVisibilityChange={handleVisibilityChange}
								/>
							))}
						</div>
					) : (
						<div
							data-testid="radio-catalog-list"
							className="flex flex-col gap-2"
						>
							{entries.map((entry) => (
								<CatalogRow
									key={entry.stationUuid}
									entry={entry}
									isImporting={importStation.isPending}
									hasPreviewError={previewErrorUuid === entry.stationUuid}
									onImport={() => importStation.mutate(entry)}
									onPreview={() => void handlePreview(entry)}
									onDetails={() => setSelectedEntry(entry)}
									onVisibilityChange={handleVisibilityChange}
								/>
							))}
						</div>
					)}

					<div ref={loadMoreRef} className="h-10" />
					{catalog.isFetchingNextPage ? (
						<p className="py-4 text-center text-caption text-sm">Loading...</p>
					) : null}
				</CollectionPageContainer>
			</div>

			<StationDetailsDialog
				entry={selectedEntry}
				onOpenChange={(isOpen) => {
					if (!isOpen) setSelectedEntry(null);
				}}
			/>
			<FilterDrawer
				isOpen={areFiltersOpen}
				filters={filters}
				countryOptions={countryOptions}
				tagOptions={tagOptions}
				viewMode={viewMode}
				onOpenChange={setAreFiltersOpen}
				onFiltersChange={setFilters}
				onViewModeChange={setViewMode}
			/>
		</div>
	);
}

function FilterDrawer({
	isOpen,
	filters,
	countryOptions,
	tagOptions,
	viewMode,
	onOpenChange,
	onFiltersChange,
	onViewModeChange,
}: {
	isOpen: boolean;
	filters: CatalogFilters;
	countryOptions: RadioCatalogOption[];
	tagOptions: RadioCatalogOption[];
	viewMode: ViewMode;
	onOpenChange: (isOpen: boolean) => void;
	onFiltersChange: React.Dispatch<React.SetStateAction<CatalogFilters>>;
	onViewModeChange: (viewMode: ViewMode) => void;
}) {
	return (
		<DialogPrimitive.Root open={isOpen} onOpenChange={onOpenChange}>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-background/50 backdrop-blur-sm" />
				<DialogPrimitive.Content
					aria-describedby="radio-filter-description"
					className="fixed top-0 right-0 z-50 flex h-dvh w-sm flex-col gap-6 border-border border-l bg-background p-6 shadow-xl outline-none"
				>
					<div className="flex items-start justify-between gap-4">
						<div className="min-w-0">
							<DialogPrimitive.Title className="font-semibold text-heading text-xl">
								Radio catalog filters
							</DialogPrimitive.Title>
							<DialogPrimitive.Description
								id="radio-filter-description"
								className="mt-1 text-caption text-sm"
							>
								Selections apply immediately.
							</DialogPrimitive.Description>
						</div>
						<DialogPrimitive.Close asChild>
							<Button type="button" variant="ghost" size="icon">
								<X className="size-4" />
								<span className="sr-only">Close</span>
							</Button>
						</DialogPrimitive.Close>
					</div>

					<div className="flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto scrollbar-hidden">
						<fieldset className="grid gap-2">
							<legend className="font-medium text-caption text-xs">
								Catalog layout
							</legend>
							<div className="flex w-fit items-center gap-1 rounded-md border border-border bg-player p-0.5">
								<ViewButton
									label="Grid view"
									isActive={viewMode === "grid"}
									onClick={() => onViewModeChange("grid")}
								>
									<Grid2X2 className="size-4" />
								</ViewButton>
								<ViewButton
									label="List view"
									isActive={viewMode === "list"}
									onClick={() => onViewModeChange("list")}
								>
									<List className="size-4" />
								</ViewButton>
							</div>
						</fieldset>
						<NativeOptionSelect
							label="Country"
							value={filters.country}
							options={countryOptions}
							isCountry
							onChange={(country) =>
								onFiltersChange((current) => ({ ...current, country }))
							}
						/>
						<NativeOptionSelect
							label="Genre"
							value={filters.tag}
							options={tagOptions}
							onChange={(tag) =>
								onFiltersChange((current) => ({ ...current, tag }))
							}
						/>
						<FormatRadioGroup
							value={filters.codec}
							onChange={(codec) =>
								onFiltersChange((current) => ({ ...current, codec }))
							}
						/>
						<QualityRadioGroup
							value={filters.minBitrate}
							onChange={(minBitrate) =>
								onFiltersChange((current) => ({ ...current, minBitrate }))
							}
						/>
					</div>
					<div className="border-border border-t pt-4">
						<Button
							type="button"
							variant="outline"
							className="w-full justify-center"
							onClick={() =>
								onFiltersChange((current) => ({
									...DEFAULT_CATALOG_FILTERS,
									q: current.q,
								}))
							}
						>
							Reset filters
						</Button>
					</div>
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}

function NativeOptionSelect({
	label,
	value,
	options,
	isCountry,
	onChange,
}: {
	label: string;
	value: string;
	options: RadioCatalogOption[];
	isCountry?: boolean;
	onChange: (value: string) => void;
}) {
	const [isOpen, setIsOpen] = useState(false);
	const [query, setQuery] = useState("");
	const ref = useRef<HTMLDivElement | null>(null);
	const allLabel = label === "Country" ? "All countries" : "All genres";
	const searchPlaceholder =
		label === "Country" ? "Search countries..." : "Search genres...";
	const searchInputId = `${label.toLowerCase()}-filter-search`;
	const selected =
		value === ALL_FILTER_VALUE
			? null
			: options.find((option) => option.name === value);
	const filteredOptions = useMemo(() => {
		const normalizedQuery = query.trim().toLowerCase();
		const uniqueOptions = dedupeOptions(options);
		if (!normalizedQuery) return uniqueOptions;
		return uniqueOptions.filter((option) =>
			option.name.toLowerCase().includes(normalizedQuery),
		);
	}, [options, query]);

	useEffect(() => {
		if (!isOpen) return;
		function handlePointerDown(event: PointerEvent) {
			if (!ref.current?.contains(event.target as Node)) {
				setIsOpen(false);
			}
		}
		document.addEventListener("pointerdown", handlePointerDown);
		return () => document.removeEventListener("pointerdown", handlePointerDown);
	}, [isOpen]);

	const [wasOpen, setWasOpen] = useState(isOpen);
	if (isOpen !== wasOpen) {
		setWasOpen(isOpen);
		if (!isOpen) setQuery("");
	}

	return (
		<div
			ref={ref}
			className="relative flex min-w-52.5 flex-col gap-1 text-caption text-xs"
		>
			<span>{label}</span>
			<button
				type="button"
				aria-label={label}
				aria-expanded={isOpen}
				className={cn(
					FILTER_TRIGGER_CLASS,
					"flex w-57.5 max-w-full items-center justify-between gap-3 px-3 text-left text-sm outline-none transition-ring focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50",
				)}
				onClick={() => setIsOpen((current) => !current)}
			>
				<span className="inline-flex min-w-0 items-center gap-2">
					{isCountry && selected ? (
						<span className="w-5 shrink-0 text-center">
							{countryFlag(selected.code)}
						</span>
					) : null}
					<span className="truncate">{selected?.name ?? allLabel}</span>
				</span>
				<ChevronDown className="size-4 shrink-0 text-caption" />
			</button>
			{isOpen ? (
				<div className="absolute top-full left-0 z-50 mt-2 flex max-h-80 w-80 flex-col overflow-hidden rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-lg">
					<label className="relative m-1 mb-2 block" htmlFor={searchInputId}>
						<Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-caption" />
						<Input
							id={searchInputId}
							className="h-10 rounded-md bg-player pl-8 text-sm"
							placeholder={searchPlaceholder}
							type="search"
							value={query}
							onChange={(event) => setQuery(event.target.value)}
						/>
					</label>
					<div className="overflow-y-auto scrollbar-hidden">
						<FilterOptionButton
							label={allLabel}
							isSelected={value === ALL_FILTER_VALUE}
							onSelect={() => {
								onChange(ALL_FILTER_VALUE);
								setIsOpen(false);
							}}
						/>
						{filteredOptions.map((option) => (
							<FilterOptionButton
								key={`${option.code}-${option.name}`}
								label={option.name}
								prefix={isCountry ? countryFlag(option.code) : undefined}
								isSelected={option.name === value}
								onSelect={() => {
									onChange(option.name);
									setIsOpen(false);
								}}
							/>
						))}
					</div>
				</div>
			) : null}
		</div>
	);
}

function FilterOptionButton({
	label,
	prefix,
	isSelected,
	onSelect,
}: {
	label: string;
	prefix?: string;
	isSelected: boolean;
	onSelect: () => void;
}) {
	return (
		<button
			type="button"
			role="option"
			aria-selected={isSelected}
			className={cn(
				"relative flex w-full items-center gap-2 rounded-sm py-2 pr-8 pl-3 text-left text-sm outline-none hover:bg-accent hover:text-accent-foreground",
				isSelected && "bg-accent text-accent-foreground",
			)}
			onClick={onSelect}
		>
			{prefix ? (
				<span className="w-5 shrink-0 text-center">{prefix}</span>
			) : null}
			<span className="min-w-0 truncate">{label}</span>
			{isSelected ? (
				<span className="absolute right-2 flex size-3.5 items-center justify-center">
					<Check className="size-4" />
				</span>
			) : null}
		</button>
	);
}

function dedupeOptions(options: RadioCatalogOption[]): RadioCatalogOption[] {
	const seen = new Set<string>();
	const out: RadioCatalogOption[] = [];
	for (const option of options) {
		const name = option.name.trim();
		if (!name) continue;
		const key = name.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		out.push({ ...option, name });
	}
	return out;
}

function buildCountryOptions(): RadioCatalogOption[] {
	const displayNames =
		typeof Intl.DisplayNames === "function"
			? new Intl.DisplayNames(["en"], { type: "region" })
			: null;
	return ISO_COUNTRY_CODES.map((code) => ({
		code,
		name: displayNames?.of(code) ?? code,
	})).sort((a, b) =>
		a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
	);
}

function FormatRadioGroup({
	value,
	onChange,
}: {
	value: string;
	onChange: (value: string) => void;
}) {
	return (
		<FilterRadioGroup
			label="Format"
			value={value}
			options={[
				{ value: ALL_FILTER_VALUE, label: "Any format" },
				{ value: "AAC", label: "AAC / AAC+" },
				{ value: "MP3", label: "MP3" },
				{ value: "OGG", label: "OGG" },
			]}
			onChange={onChange}
		/>
	);
}

function QualityRadioGroup({
	value,
	onChange,
}: {
	value: string;
	onChange: (value: string) => void;
}) {
	return (
		<FilterRadioGroup
			label="Quality"
			value={value}
			options={[
				{ value: ALL_FILTER_VALUE, label: "Any bitrate" },
				{ value: "96", label: "96 kbps+" },
				{ value: "128", label: "128 kbps+" },
				{ value: "192", label: "192 kbps+" },
				{ value: "320", label: "320 kbps+" },
			]}
			onChange={onChange}
		/>
	);
}

function FilterRadioGroup({
	label,
	value,
	options,
	onChange,
}: {
	label: string;
	value: string;
	options: { value: string; label: string }[];
	onChange: (value: string) => void;
}) {
	const radioGroupName = `radio-filter-${label.toLowerCase()}`;

	return (
		<div className="flex flex-col gap-2">
			<span className="text-caption text-xs">{label}</span>
			<div role="radiogroup" aria-label={label} className="grid gap-2">
				{options.map((option) => (
					<label
						key={option.value}
						className={cn(
							"flex h-11 items-center gap-3 rounded-lg border border-border bg-player px-3 text-left text-player-foreground text-sm outline-none transition-ring hover:border-player-control-primary focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50",
							option.value === value &&
								"border-player-control-primary bg-player-pill",
						)}
					>
						<input
							type="radio"
							name={radioGroupName}
							value={option.value}
							checked={option.value === value}
							className="sr-only"
							onChange={() => onChange(option.value)}
						/>
						<span
							className={cn(
								"flex size-4 shrink-0 items-center justify-center rounded-full border border-caption",
								option.value === value && "border-player-control-primary",
							)}
						>
							<span
								className={cn(
									"size-2 rounded-full bg-transparent",
									option.value === value && "bg-player-control-primary",
								)}
							/>
						</span>
						<span>{option.label}</span>
					</label>
				))}
			</div>
		</div>
	);
}

function ViewButton({
	label,
	isActive,
	children,
	onClick,
}: {
	label: string;
	isActive: boolean;
	children: React.ReactNode;
	onClick: () => void;
}) {
	return (
		<Button
			type="button"
			size="icon"
			variant={isActive ? "default" : "ghost"}
			aria-label={label}
			aria-pressed={isActive}
			className="size-8 rounded-sm"
			onClick={onClick}
		>
			{children}
		</Button>
	);
}

function CatalogCard({
	entry,
	isImporting,
	hasPreviewError,
	onImport,
	onPreview,
	onDetails,
	onVisibilityChange,
}: CatalogEntryProps) {
	const ref = useVisibilityRef(entry.stationUuid, onVisibilityChange);
	const tags = visibleTags(entry);

	return (
		<ContextMenu>
			<ContextMenuTrigger asChild>
				<button
					type="button"
					ref={ref}
					data-testid={`radio-catalog-card-${entry.stationUuid}`}
					onClick={onPreview}
					className={cn(
						"group relative flex min-w-0 cursor-pointer items-start gap-4 rounded-md border border-border bg-card/45 p-4 text-left hover:bg-card/65",
						CARD_INTERACTION_CLASS,
					)}
				>
					<CatalogArtwork faviconUrl={entry.faviconUrl} name={entry.name} />
					<CardHealthStatus status={entry.healthStatus} />
					<div className="flex min-w-0 flex-1 flex-col gap-3 pr-12">
						<div className="min-w-0 space-y-2">
							<h2
								className="truncate font-semibold text-base text-heading"
								title={entry.name}
							>
								{entry.name}
							</h2>
							<div className="flex min-w-0 flex-col gap-1 text-foreground text-xs">
								<p className="inline-flex min-w-0 items-center gap-2">
									<MapPin className="size-3.5 shrink-0 text-caption" />
									<span className="truncate">{formatCardLocation(entry)}</span>
								</p>
								<p className="inline-flex min-w-0 items-center gap-2">
									<AudioLines className="size-3.5 shrink-0 text-caption" />
									<span className="truncate">{formatQuality(entry)}</span>
								</p>
							</div>
						</div>
						<div className="flex flex-wrap gap-2">
							{tags.map((tag) => (
								<MetaChip key={tag}>{tag}</MetaChip>
							))}
						</div>
						{hasPreviewError ? (
							<p className="mt-auto text-destructive text-xs">
								Live playback unavailable.
							</p>
						) : null}
					</div>
				</button>
			</ContextMenuTrigger>
			<CatalogContextMenu
				isImporting={isImporting}
				onImport={onImport}
				onDetails={onDetails}
			/>
		</ContextMenu>
	);
}

function CatalogRow({
	entry,
	isImporting,
	hasPreviewError,
	onImport,
	onPreview,
	onDetails,
	onVisibilityChange,
}: CatalogEntryProps) {
	const ref = useVisibilityRef(entry.stationUuid, onVisibilityChange);
	const tags = visibleTags(entry);

	return (
		<ContextMenu>
			<ContextMenuTrigger asChild>
				<button
					type="button"
					ref={ref}
					data-testid={`radio-catalog-card-${entry.stationUuid}`}
					onClick={onPreview}
					className="group grid min-w-0 cursor-pointer grid-cols-media gap-3 rounded-lg border border-border bg-card/45 p-3 text-left shadow-sm outline-none transition duration-300 ease-out hover:-translate-y-1 hover:border-player-control-primary/60 hover:bg-card/65 hover:shadow-lg focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 md:grid-cols-media-actions"
				>
					<CatalogArtwork
						faviconUrl={entry.faviconUrl}
						name={entry.name}
						small
					/>
					<div className="min-w-0">
						<h2 className="truncate font-semibold text-heading text-sm">
							{entry.name}
						</h2>
						<p className="mt-1 flex min-w-0 items-center gap-1 truncate text-foreground text-xs">
							<MapPin className="size-3.5 shrink-0 text-caption" />
							<span className="truncate">{formatLocation(entry)}</span>
						</p>
						{hasPreviewError ? (
							<p className="mt-1 text-destructive text-xs">
								Live playback unavailable.
							</p>
						) : null}
					</div>
					<div className="col-span-2 flex flex-wrap items-center gap-2 md:col-span-1 md:justify-end">
						<MetaChip>{formatQuality(entry)}</MetaChip>
						<HealthChip status={entry.healthStatus} />
						{tags.map((tag) => (
							<MetaChip key={tag}>{tag}</MetaChip>
						))}
						{extraTagCount(entry) > 0 ? (
							<MetaChip>+{extraTagCount(entry)}</MetaChip>
						) : null}
					</div>
				</button>
			</ContextMenuTrigger>
			<CatalogContextMenu
				isImporting={isImporting}
				onImport={onImport}
				onDetails={onDetails}
			/>
		</ContextMenu>
	);
}

type CatalogEntryProps = {
	entry: RadioSearchResult;
	isImporting: boolean;
	hasPreviewError: boolean;
	onImport: () => void;
	onPreview: () => void;
	onDetails: () => void;
	onVisibilityChange: (stationUuid: string, isVisible: boolean) => void;
};

function CatalogContextMenu({
	isImporting,
	onImport,
	onDetails,
}: {
	isImporting: boolean;
	onImport: () => void;
	onDetails: () => void;
}) {
	return (
		<ContextMenuContent>
			<ContextMenuItem disabled={isImporting} onSelect={onImport}>
				<Radio className="size-4" />
				Import / Add to radio stations
			</ContextMenuItem>
			<ContextMenuSeparator />
			<ContextMenuItem onSelect={onDetails}>
				<Info className="size-4" />
				Details
			</ContextMenuItem>
		</ContextMenuContent>
	);
}

function CatalogArtwork({
	faviconUrl,
	name,
	small,
}: {
	faviconUrl?: string;
	name: string;
	small?: boolean;
}) {
	const [hasImageError, setHasImageError] = useState(false);
	const sizeClass = small ? "size-14 rounded-md" : "h-24 w-24 rounded-lg";
	if (!faviconUrl || hasImageError) {
		return (
			<div
				className={cn(
					"flex shrink-0 items-center justify-center border border-border bg-player-artwork text-player-control-primary",
					sizeClass,
				)}
			>
				<Headphones className={small ? "size-6" : "size-11"} />
			</div>
		);
	}
	return (
		<img
			alt={name}
			className={cn(
				"shrink-0 border border-border bg-muted object-cover",
				sizeClass,
			)}
			src={faviconUrl}
			onError={() => setHasImageError(true)}
			onLoad={(event) => {
				if (!isUsableCatalogArtwork(event.currentTarget)) {
					setHasImageError(true);
				}
			}}
		/>
	);
}

function isUsableCatalogArtwork(image: HTMLImageElement) {
	const { naturalHeight, naturalWidth } = image;
	if (
		naturalWidth < MIN_ARTWORK_IMAGE_SIZE ||
		naturalHeight < MIN_ARTWORK_IMAGE_SIZE
	) {
		return false;
	}
	const aspectRatio =
		Math.max(naturalWidth, naturalHeight) /
		Math.min(naturalWidth, naturalHeight);
	return aspectRatio <= MAX_ARTWORK_ASPECT_RATIO;
}

function HealthChip({ status }: { status?: string }) {
	const normalized = status ?? "unknown";
	const labels = {
		healthy: "Healthy",
		broken: "Broken",
		unknown: "Unknown",
	} as const;
	const Icon =
		normalized === "healthy"
			? BadgeCheck
			: normalized === "broken"
				? ShieldAlert
				: ShieldQuestion;
	return (
		<MetaChip
			className={cn(
				normalized === "healthy" && "text-success",
				normalized === "broken" && "text-destructive",
			)}
		>
			<Icon className="size-3.5" />
			{labels[normalized as keyof typeof labels] ?? labels.unknown}
		</MetaChip>
	);
}

function CardHealthStatus({ status }: { status?: string }) {
	const normalized = status ?? "unknown";
	const label =
		normalized === "healthy"
			? "Healthy"
			: normalized === "broken"
				? "Broken"
				: "Unknown";
	return (
		<span className="absolute top-4 right-4 inline-flex items-center gap-2 text-caption text-xs">
			<span
				className={cn(
					"size-2 rounded-full",
					normalized === "healthy" && "bg-success text-success",
					normalized === "broken" && "bg-destructive text-destructive",
					normalized !== "healthy" &&
						normalized !== "broken" &&
						"bg-caption text-caption",
				)}
			/>
			{label}
		</span>
	);
}

function MetaChip({
	children,
	className,
}: {
	children: React.ReactNode;
	className?: string;
}) {
	return (
		<span
			className={cn(
				"inline-flex h-6 max-w-full items-center gap-1 truncate rounded-full border border-border bg-player-pill px-2 text-caption text-xs",
				className,
			)}
		>
			{children}
		</span>
	);
}

function StationDetailsDialog({
	entry,
	onOpenChange,
}: {
	entry: RadioSearchResult | null;
	onOpenChange: (isOpen: boolean) => void;
}) {
	return (
		<DialogPrimitive.Root open={entry != null} onOpenChange={onOpenChange}>
			<DialogPrimitive.Portal>
				<DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-background/80 backdrop-blur-sm" />
				<DialogPrimitive.Content
					aria-describedby="radio-details-description"
					className="-translate-x-1/2 -translate-y-1/2 fixed top-1/2 left-1/2 z-50 flex max-h-dialog-height w-dialog-width max-w-2xl flex-col gap-6 overflow-y-auto rounded-lg border border-border bg-background p-6 shadow-xl outline-none"
				>
					{entry ? (
						<>
							<div className="flex items-start gap-4">
								<CatalogArtwork
									faviconUrl={entry.faviconUrl}
									name={entry.name}
								/>
								<div className="min-w-0 flex-1">
									<DialogPrimitive.Title className="truncate font-semibold text-2xl text-heading">
										{entry.name}
									</DialogPrimitive.Title>
									<DialogPrimitive.Description
										id="radio-details-description"
										className="mt-2 text-foreground text-sm"
									>
										{formatLocation(entry)}
									</DialogPrimitive.Description>
									<div className="mt-3 flex flex-wrap gap-2">
										<MetaChip>{formatQuality(entry)}</MetaChip>
										<HealthChip status={entry.healthStatus} />
									</div>
								</div>
								<DialogPrimitive.Close asChild>
									<Button type="button" variant="ghost" size="icon">
										<X className="size-4" />
										<span className="sr-only">Close</span>
									</Button>
								</DialogPrimitive.Close>
							</div>
							<dl className="grid gap-3 text-sm sm:grid-cols-2">
								<DetailItem label="Country" value={entry.country} />
								<DetailItem label="Language" value={entry.language} />
								<DetailItem label="Votes" value={String(entry.votes ?? "")} />
								<DetailItem label="Last checked" value={entry.lastCheckedAt} />
								<DetailItem
									label="Last successful"
									value={entry.lastSuccessfulAt}
								/>
								<DetailItem label="Homepage" value={entry.homepageUrl} />
							</dl>
							<div>
								<h3 className="mb-2 font-medium text-heading text-sm">Tags</h3>
								<div className="flex flex-wrap gap-2">
									{entry.tags.length > 0 ? (
										entry.tags.map((tag) => (
											<MetaChip key={tag}>{tag}</MetaChip>
										))
									) : (
										<p className="text-caption text-sm">No tags.</p>
									)}
								</div>
							</div>
						</>
					) : null}
				</DialogPrimitive.Content>
			</DialogPrimitive.Portal>
		</DialogPrimitive.Root>
	);
}

function DetailItem({ label, value }: { label: string; value?: string }) {
	return (
		<div className="min-w-0 rounded-md border border-border bg-card/35 p-3">
			<dt className="text-caption text-xs">{label}</dt>
			<dd className="mt-1 truncate text-foreground text-sm">
				{value || "Unknown"}
			</dd>
		</div>
	);
}

function SkeletonCard() {
	return (
		<div className="flex min-h-44 gap-4 rounded-lg border border-border bg-card/35 p-4">
			<div className="size-24 shrink-0 animate-pulse rounded-lg bg-muted" />
			<div className="flex flex-1 flex-col gap-3">
				<div className="h-5 w-2/3 animate-pulse rounded-sm bg-muted" />
				<div className="h-4 w-1/2 animate-pulse rounded-sm bg-muted" />
				<div className="flex flex-wrap gap-2">
					<div className="h-7 w-20 animate-pulse rounded-full bg-muted" />
					<div className="h-7 w-24 animate-pulse rounded-full bg-muted" />
				</div>
			</div>
		</div>
	);
}

function useVisibilityRef(
	stationUuid: string,
	onVisibilityChange: (stationUuid: string, isVisible: boolean) => void,
) {
	const ref = useRef<HTMLButtonElement | null>(null);

	useEffect(() => {
		const node = ref.current;
		if (!node || typeof IntersectionObserver === "undefined") {
			onVisibilityChange(stationUuid, true);
			return () => onVisibilityChange(stationUuid, false);
		}
		const observer = new IntersectionObserver(
			([record]) => onVisibilityChange(stationUuid, record.isIntersecting),
			{ threshold: 0.2 },
		);
		observer.observe(node);
		return () => {
			observer.disconnect();
			onVisibilityChange(stationUuid, false);
		};
	}, [stationUuid, onVisibilityChange]);

	return ref;
}

function selectedFilterValue(value: string): string | undefined {
	return value === ALL_FILTER_VALUE ? undefined : value;
}

function countryFlag(code?: string): string {
	if (!code || code.length !== 2) return "";
	const upper = code.toUpperCase();
	const first = upper.charCodeAt(0);
	const second = upper.charCodeAt(1);
	if (first < 65 || first > 90 || second < 65 || second > 90) return "";
	return String.fromCodePoint(0x1f1e6 + first - 65, 0x1f1e6 + second - 65);
}

function visibleTags(entry: RadioSearchResult): string[] {
	return entry.tags.slice(0, 2);
}

function extraTagCount(entry: RadioSearchResult): number {
	return Math.max(0, entry.tags.length - visibleTags(entry).length);
}

function formatCardLocation(entry: RadioSearchResult): string {
	return entry.country || "Global";
}

function formatLocation(entry: RadioSearchResult): string {
	return (
		[entry.country, entry.language].filter(Boolean).join(" / ") || "Global"
	);
}

function formatQuality(entry: RadioSearchResult): string {
	const codec = normalizeCodec(entry.codec);
	const bitrate = entry.bitrate ? `${entry.bitrate} kbps` : null;
	return [codec, bitrate].filter(Boolean).join(" ") || "Quality unavailable";
}

function normalizeCodec(codec?: string): string | null {
	const normalized = codec?.trim().toUpperCase();
	if (!normalized || normalized === "UNKNOWN") return null;
	return normalized;
}
