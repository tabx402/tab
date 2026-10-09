import React from 'react';
import '@fontsource/dm-sans/400.css';
import '@fontsource/dm-sans/500.css';
import {createRoot} from 'react-dom/client';
import '../src/styles.css';
import {AgentWizard} from '../src/components/AgentWizard';
createRoot(document.getElementById('root')!).render(<div className="account-flow"><h1>create your agent.</h1><AgentWizard sponsorshipAvailable={new URLSearchParams(location.search).get("sponsored")==="true"} pending={false} onCancel={()=>{}} onCreate={async(input)=>{document.body.dataset.created=JSON.stringify(input);throw Error('wallet confirmation is required');}}/></div>);
