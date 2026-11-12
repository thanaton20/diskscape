import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { getLocale } from './lib/i18n';

document.documentElement.lang = getLocale();

export default mount(App, { target: document.getElementById('app')! });
