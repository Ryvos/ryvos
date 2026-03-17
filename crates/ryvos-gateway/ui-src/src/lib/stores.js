import { writable } from 'svelte/store';

export const authenticated = writable(false);
export const currentRoute = writable('dashboard');
export const sessions = writable([]);
export const metrics = writable({});
