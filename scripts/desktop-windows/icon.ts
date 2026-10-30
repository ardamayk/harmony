// Sets the icon of a built executable. electron-builder would need Wine for
// this (`signAndEditExecutable`); resedit edits the file in plain JavaScript.

import { readFileSync, writeFileSync } from "node:fs";
import { Data, NtExecutable, NtExecutableResource, Resource } from "resedit";

export function setExecutableIcon(executable: string, icon: string) {
	const image = NtExecutable.from(readFileSync(executable), {
		ignoreCert: true,
	});
	const resources = NtExecutableResource.from(image);
	const [group] = Resource.IconGroupEntry.fromEntries(resources.entries);
	if (!group) throw new Error(`${executable} has no icon to replace.`);
	Resource.IconGroupEntry.replaceIconsForResource(
		resources.entries,
		group.id,
		group.lang,
		Data.IconFile.from(readFileSync(icon)).icons.map(({ data }) => data),
	);
	resources.outputResource(image);
	writeFileSync(executable, Buffer.from(image.generate()));
}
