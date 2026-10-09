"""Rebuild the requested social archive around Tab's jobs and bounded branches."""
from pathlib import Path
import hashlib
import html
import json
import re
import shutil
import zipfile

BASE=Path('/home/ubuntu/Videos/codex')
SOURCE=BASE/'tab-brand-pack-2026-10-03'
DEST=BASE/'tab-brand-pack-2026-10-04'

TWEETS=[
('official-home',"a home for your x402 agents.\n\ngive one a job, define what counts as done, and keep its work and budget in view.\n\ntabagents.io"),
('official-first-job',"start with a small job.\n\nchoose an agent, a deadline, an acceptance rule and a budget. the terms stay attached to the work."),
('official-garden',"each agent has a place in the garden.\n\nits runs grow leaves. jobs grow buds. accepted work adds mint growth. confirmed payments add amber berries."),
('official-gas',"BNB pays agent registration gas.\n\njob funding, escrow actions and paid reports have their own wallet approvals and costs. you see them before signing."),
('official-task-terms',"a job starts with an agreement:\n\nwhat to deliver\nwho does it\nwhen it is due\nwhat the budget allows\nhow it is accepted"),
('official-review',"for work that needs judgment, the buyer reviews the evidence and approves the remaining reward.\n\nthe approval names the exact evidence being accepted."),
('official-observation',"a block observation has a narrower rule.\n\nthe executor records a confirmed BNB Smart Chain block and its block hash. the buyer reviews that exact evidence before releasing the remaining reward."),
('official-branches',"an agent can give part of a job to another agent.\n\nthe branch gets its own budget, deadline and permissions. its money comes out of the parent job."),
('official-budget',"a 10 USDT job can allocate 3 USDT to a specialist.\n\nthat leaves 7 available to the parent. another branch has to fit inside what remains."),
('official-inheritance',"a branch can narrow its service permissions, lower its per-call cap and finish earlier.\n\nit cannot expand the permissions or budget it received."),
('official-roots',"the garden keeps delegated work connected.\n\nfine roots link the agents involved. open the job to see its terms, branch allocations and evidence."),
('official-unused',"cancel an unused branch and its remaining funds return to the parent job.\n\nclose the root and its remaining funds return to the buyer."),
('official-pause',"pause the root job to stop new escrow spending, delegation and acceptance across its branches.\n\nunused branches can still be closed."),
('official-evidence',"a completed run, submitted evidence and accepted work each have their own record.\n\npayment appears once its transaction is confirmed."),
('official-drafts',"prepare the job before funding it.\n\nchoose the agent, write the terms and allocate draft branches. an unfunded budget stays clearly marked."),
('official-payments',"x402 reports use the existing wallet-approved payment flow.\n\npurchases from job escrow require a connected merchant invoice adapter. each route shows its own permissions."),
('official-builder',"builders can use an agent key to read its jobs, collect chain evidence and prepare branches.\n\nthe key does not sign wallet transactions."),
('official-scoped-services',"an escrow service payment needs a merchant-signed invoice naming the job, service, request, amount and expiry.\n\nthe contract checks its scope and per-call cap before paying."),
('official-delivery',"a service payment records where the funds went.\n\nthe result still needs to arrive and satisfy the job's acceptance rule."),
('official-timeouts',"buyer-review jobs allow one day after the deadline for acceptance.\n\nif that window expires, the unused funds can be returned. active child branches must close first."),
('official-owner',"your agent's registration stays owned by your wallet.\n\njob agreements have their own terms and escrow records. you can follow both from tab."),
('official-read-only',"a chain check can run without spending USDT.\n\na paid job adds a separate agreement: who requested the work, what it must deliver and how payment is released."),
('official-schedule',"keep the simple jobs simple.\n\nmanual runs, hourly checks and daily observations remain available. use a job agreement when another agent is doing work for you."),
('official-tools',"choose the tools before the work starts.\n\nbranches inherit a subset. service payments also need an approved recipient and a valid invoice."),
('official-record',"open an agent and follow the work.\n\nterms, deadlines, evidence, branch budgets and confirmed transactions stay connected to the job."),
('official-small-example',"one job, two agents.\n\none gathers the observation. the other reviews and assembles the result. each works inside the budget allocated to it."),
('official-no-double-spend',"branch budgets are reserved when they are allocated.\n\ntwo agents working at once cannot each spend the same remaining funds."),
('official-observation-limit',"a chain observation records a confirmed block and its hash.\n\nreports, opinions and recommendations use buyer review. the acceptance rule is chosen with the job."),
('official-start',"give the first agent something small to do.\n\nread its result. set a schedule if it helps. create a bounded job when you want another agent involved.\n\ntabagents.io"),
('official-receipts',"accepted work leaves a receipt with the evidence hash and reward.\n\nbranches keep their own records, and the root keeps the total spending and refunds."),
('founder-job',"i want to open an agent and see what it owes someone, what it has delivered and how much it can still spend. that is the job view in tab."),
('founder-garden',"the garden is a way to read the work.\n\na bud means a job is open. a root means work was delegated. the details are one click away."),
('founder-budget',"giving an agent a budget gets more interesting when it hires another one.\n\nwe made each branch take its money from the parent and inherit tighter limits."),
('founder-acceptance',"before an agent starts a paid job, i want the acceptance rule written down. otherwise everyone gets to have a different definition of done."),
('founder-proof',"we started with a small thing the chain can actually check: a recent block observation.\n\nmore complicated work goes through buyer review."),
('founder-caps',"the agent can be creative inside the job. the budget still has arithmetic."),
('founder-first-visit',"the first visit should be ordinary.\n\nmake an agent, give it a job, choose its tools. if it needs help, give another agent a small branch of that job."),
('founder-history',"a week later, i should still be able to see which agent did the work, which evidence was accepted and where the money went."),
('founder-control',"an agent hiring three more agents should not turn one small permission into four open wallets.\n\nthe branches share the original budget."),
('founder-calm',"the birds can stay calm.\n\nthe contracts underneath have to keep track of the money."),
]

THREADS=[
('a-home-for-your-agents',[
"a home for your x402 agents.\n\ntab brings their jobs, tools, spending limits and work history into one place.\n\ntabagents.io",
"start with a simple agent.\n\ngive it a name, choose what it should check, and run it manually or on a schedule. tab covers its registration gas.",
"when you want work from another agent, create a job.\n\nwrite the deliverable, choose an acceptance rule, set a deadline and review the budget before funding.",
"the assigned agent can give a specialist a branch of that job.\n\nthe branch takes its budget from the parent and inherits narrower service permissions, caps and deadlines.",
"general work goes to the buyer for review.\n\na recent block observation can use the onchain block-hash check. the rule is attached to the job before the work starts.",
"the garden shows the relationships.\n\nopen jobs are buds. accepted work adds mint growth. confirmed payments add amber berries. fine roots connect delegated agents.",
"open the job to see what was promised, what was delivered and how the budget was used.\n\nstart small.\n\ntabagents.io",
]),
('one-budget-many-agents',[
"an agent can hire another agent in tab.\n\nthe permission and money for that work come from the job it already has.",
"say a job has 10 USDT.\n\nit allocates 3 to a specialist. the parent now has 7 available. a second branch has to fit inside those remaining funds.",
"the specialist receives its own terms.\n\na smaller scope, a per-call cap and a deadline within the parent's deadline. it can delegate again within those same bounds.",
"escrow service payments require a merchant-signed invoice for the permitted service and request.\n\nthe contract checks the amount, expiry, signature and replay protection.",
"a branch cannot create more budget by running in parallel. allocations subtract from the parent before any other branch can use those funds.",
"pause the root to stop new escrow actions across the tree.\n\nclose unused branches from the leaves upward. their remaining funds return to their parents.",
"the garden draws the connections. the job view shows the numbers.\n\none agreement can become a small team with a bounded budget.\n\ntabagents.io",
]),
('what-counts-as-done',[
"a paid job needs a definition of done.\n\nin tab, the acceptance rule is part of the terms the buyer funds.",
"for general work, the agent submits evidence.\n\nthe buyer reviews it and approves the exact evidence hash. acceptance releases the remaining reward to the assigned executor.",
"for a recent block observation, the contract checks the block number and hash against the chain.\n\nthe observation must meet the minimum block and arrive before the job deadline.",
"that proof is deliberately narrow.\n\na research report still needs judgment. the buyer-review route makes that responsibility explicit.",
"service costs and rewards come from the same job budget.\n\nwhen a connected service is paid, less remains for the agent's reward or further delegation.",
"submitted buyer-review work has a one-day review window after its deadline.\n\nexpired unused funds can be returned once child branches have closed.",
"terms, evidence and confirmed payments stay in the job history.\n\nyou can come back and see what was accepted.\n\ntabagents.io",
]),
('reading-the-garden',[
"the garden in tab grows from agent records.\n\neach plant belongs to an agent. its shape stays consistent when you return.",
"completed runs add leaves. tools have their own blossoms. failed runs leave coral marks. confirmed payments add amber berries.",
"jobs sit in the same garden.\n\nan open agreement adds a bud. accepted work adds mint growth. an unfunded draft remains visibly unfunded.",
"when one agent delegates to another, a fine root connects their plants.\n\nopen the job to follow the branch and its inherited budget.",
"the detail view carries the terms: deadline, acceptance rule, available budget, evidence and transaction history.\n\nthe plant gives you a reason to look closer.",
"simple scheduled checks still have their place.\n\njob agreements are there when you want another agent to deliver work under terms you can inspect.",
"a home for your x402 agents.\n\ngive one a small job and follow what it does.\n\ntabagents.io",
]),
]

ARTICLES=[
('a-home-for-your-agents',"""a home for your agents

more software can now call a model, read chain data and pay for a service. once several agents are involved, the ordinary questions start to matter. what is each one doing? who asked for it? how much can it spend? where did the result go?

tab gives that work a place to live.

on the first visit, you create an agent, choose its tools and give it a simple purpose. it can check a wallet or observe the chain. you can run it yourself or choose an hourly or daily schedule. registration belongs to your wallet, with the registration gas covered by tab.

the agent has a plant in the garden. completed runs add leaves, tools have blossoms, and failures leave coral marks. you can open the agent and inspect the record behind that growth.

when you want another agent to do work, you create a job agreement. the terms name the deliverable, the assigned agent, the deadline, the acceptance rule and the budget. a draft is clearly unfunded. funding puts the agreed tokens in the job escrow.

the assigned agent can allocate a branch to a specialist. that branch gets a portion of the existing budget, with service permissions, a per-call cap and a deadline inside the parent's limits. unused branch funds return to the parent when the branch closes.

acceptance depends on the rule you chose. general work goes to the buyer for review. a recent block observation can use a direct onchain block-hash check. that check proves the observation; a report or recommendation still needs a buyer's judgment.

jobs appear as buds in the garden. accepted work adds mint growth, confirmed payments add amber berries, and fine roots connect delegated agents. the detail view shows the agreement and its evidence.

the existing x402 report flow remains a wallet-approved purchase. spending from job escrow uses a separate merchant-invoice integration, available where a service supports it. the two routes have their own approvals and records.

start with a small job. read the result, check the terms, and decide whether it should run again.

tabagents.io

[media: header/PFP pair 01. follow with the actual garden and a job detail capture after release. show the funding label and acceptance rule. use real account data with private details removed; label any demonstration as an example.]
"""),
('the-first-job',"""the first job

someone wants a confirmed chain observation. it is a small request, with a clear place to start and a result another person can check.

in tab, the buyer chooses the agent, writes the deliverable and sets a deadline. the job also has a budget and an acceptance rule. those terms stay attached to it when it is funded.

buyer review is used for work that needs judgment: a report, a summary, or evidence assembled for a particular question. the agent submits its evidence, and the buyer approves that exact evidence before the remaining reward is released.

a block observation records a confirmed BNB Smart Chain block number and hash. the executor submits that evidence before the deadline. the buyer reviews the exact evidence before releasing the remaining reward.

a recorded block hash does not evaluate a recommendation or establish that a report is useful. the buyer reviews whether the submitted evidence meets the job terms.

the agent can also hire a specialist. if the job holds 10 USDT and it allocates 3 to a branch, 7 remains available to the parent. the specialist has its own deadline, per-call cap and service permissions. its branch cannot expand the authority it received.

any connected service purchases reduce the same budget. accepted specialist work releases that branch's remaining reward. the parent can then submit its own result, once its child branches have closed.

the buyer can pause the root to stop new escrow actions throughout the tree. cancellation works from the leaves upward so that funds are accounted for before a parent closes. a submitted buyer-review job has a one-day acceptance window after its deadline; once that window expires, unused funds can be returned.

the garden shows the open job as a bud and the agents' relationship as a root. opening it brings up the terms, evidence and budget. an accepted job keeps its evidence hash and confirmed transaction in the record.

the first job can stay small enough to understand from beginning to end.

tabagents.io

[media: header/PFP pair 02. use three real product frames after release: create job, evidence, confirmed acceptance. show the same job throughout. no illustrative transfer should be presented as a customer payment.]
"""),
('when-software-asks-to-spend',"""when software asks to spend

giving an agent access to a paid service creates a practical question. how far does that permission go when the agent asks someone else to help?

tab ties the authority to a job and its funded budget.

the buyer funds terms with an assigned executor, a deadline and an acceptance rule. the executor can allocate part of those funds to another agent. that branch gets its own agreement within the parent's bounds.

a 3 USDT branch comes out of the parent's available funds at allocation. service permissions can shrink, the per-call cap can fall, and the deadline can move earlier. each further branch follows the same rules. concurrent agents cannot each reserve the same remaining funds.

for purchases from escrow, the permitted service has a fixed recipient and service identifier. its merchant signs an invoice binding the job, service, request, amount, expiry and nonce. the contract verifies that invoice, checks the branch's permissions and cap, and rejects replayed invoices before transferring tokens.

this requires a connected merchant adapter. tab's existing wallet-approved x402 reports remain a separate route. a service supporting one payment route does not automatically support the other.

payment and delivery each have their own responsibility. an invoice names the purchase, and the payment proves where the money went. the delivered result still has to satisfy the job's acceptance rule.

the buyer can pause the root job. new escrow spending, delegation and acceptance then stop throughout its branches. unused branches remain cancellable, allowing their funds to return to the parent. child branches close before their parent so that no active allocation is left behind.

the job view keeps available funds, branch allocations, costs, rewards and refunds connected. funds supplied by the buyer stay distinct from money earned through accepted work.

in the garden, a root links the agents involved. it gives you a visible relationship to inspect. the actual authority lives in the job terms and the contract that enforces them.

an agent can ask for help while remaining inside the budget it received.

tabagents.io

[media: header/PFP pair 04. use a real branch budget view after release. a simple diagram may show 10 USDT split into 7 available and 3 allocated; label it as an example. show invoice support only for connected merchants.]
"""),
('a-place-to-look-back',"""a place to look back

a result arrives, gets read and disappears into another conversation. a week later, someone asks which service supplied it, who approved it, or how much it cost.

tab keeps those questions close to the agent's work.

each agent has its own plant in the garden. runs add leaves and tools have their own blossoms. open jobs are buds, accepted work adds mint growth, and amber berries represent confirmed payments. delegated jobs connect agents through fine roots.

the plant is the entrance to a record. opening a job shows its terms, assigned agent, deadline, acceptance rule and budget. branches retain their own allocations and evidence. the root keeps the overall spending and refund totals.

an agreement starts as an unfunded draft. that label remains until the funding transaction is verified. collecting evidence does not turn a draft into paid work. submission, acceptance and settlement each have their own state.

for buyer-reviewed work, the acceptance transaction identifies the evidence hash being approved. for recent block observations, the escrow checks the observation against the chain. both leave a record that can be inspected later.

money has its own history. an allocated branch takes funds from its parent. a service payment reduces the amount available for rewards. cancelling an unused branch returns its remaining funds to the parent. cancelling the root returns its remaining funds to the buyer.

privacy is chosen with the work. public activity omits detailed outputs and private instructions. tab hides public job relationships when participants or the job are private. funded contract transactions and their hashes remain visible onchain, so private app activity does not make an onchain transfer secret.

simple agent checks still work without a paid agreement. a daily observation can stay a daily observation. a job adds terms when another agent is delivering work for you.

when you return, the garden has the same agents in it. their records explain what grew and which work is still open.

tabagents.io

[media: header/PFP pair 06. use the garden plus the job history after release. obscure wallet/account details in private screenshots. an illustration can carry the introduction; financial claims should use confirmed records.]
"""),
]

def archive(path: Path, folder: Path) -> None:
    with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as z:
        for file in sorted(folder.rglob('*')):
            if file.is_file():z.write(file,file.relative_to(folder))

def main() -> None:
    DEST.mkdir(parents=True,exist_ok=True)
    for folder in ('headers','pfps','previews'):
        shutil.copytree(SOURCE/folder,DEST/folder,dirs_exist_ok=True)
    for folder in ('tweets','threads','articles'):
        target=DEST/folder
        if target.exists():shutil.rmtree(target)
        target.mkdir()
    for i,(slug,body) in enumerate(TWEETS,1):
        body=body.lower()
        assert len(body)<=280,(i,len(body))
        assert '—' not in body and '--' not in body and body==body.lower()
        (DEST/'tweets'/f'tweet.{i:02d}-{slug}.txt').write_text(body+'\n')
    for i,(slug,posts) in enumerate(THREADS,1):
        target=DEST/'threads'/f'thread.{i:02d}-{slug}'
        target.mkdir()
        all_posts=[]
        for j,body in enumerate(posts,1):
            body=f'{j}/{len(posts)}\n\n{body}'.lower()
            assert len(body)<=280,(slug,j,len(body))
            assert '—' not in body and '--' not in body and body==body.lower()
            (target/f'post.{j:02d}.txt').write_text(body+'\n')
            all_posts.append(body)
        (DEST/'threads'/f'thread.{i:02d}-{slug}.txt').write_text('\n\n\n'.join(all_posts)+'\n')
    for i,(slug,body) in enumerate(ARTICLES,1):
        body=body.lower()
        assert '—' not in body and '--' not in body and body==body.lower()
        (DEST/'articles'/f'article.{i:02d}-{slug}.txt').write_text(body)
    status={
        'release':'2026-10-04 jobs and bounded delegation',
        'publication_status':'hold for public interface release and verified escrow deployment',
        'standalone_tweets':len(TWEETS),'threads':len(THREADS),
        'thread_posts':sum(len(posts) for _,posts in THREADS),'articles':len(ARTICLES),
        'visual_pairs':6,'visual_identity':'original six matched bird pairs retained',
        'scope':'buyer-reviewed jobs, recent-block observations, bounded budget branches',
        'merchant_status':'escrow service payments require connected merchant invoice adapters; existing wallet-approved x402 is separate',
        'metrics':'no user, revenue, volume or partnership claims',
    }
    (DEST/'release.json').write_text(json.dumps(status,indent=2)+'\n')
    # Existing artwork carries neutral, still-valid positioning. Preserve every
    # numbered original rather than changing the identity for a feature release.
    page=(SOURCE/'index.html').read_text()
    page=page.replace('40 individual tweet drafts','40 rewritten tweets + 4 threads')
    page=page.replace('40 tweets','40 rewritten tweets + 4 threads')
    page=page.replace('tab / social identity / 03 oct 2026','tab / social identity / 04 oct 2026')
    page=page.replace('Pinterest pin links identify references; search links are labelled as searches.','Media placements use the coordinated bird pairs and product captures after release.')
    tweet_links=''.join(f'<li><a href="tweets/tweet.{i:02d}-{slug}.txt">tweet.{i:02d}-{slug}.txt</a></li>' for i,(slug,_) in enumerate(TWEETS,1))
    tweet_section='<details><summary>40 rewritten tweet files</summary><p>30 official-account posts and 10 founder posts. Each file contains one post under 280 characters. Hold for the coordinated release.</p><ul>'+tweet_links+'</ul></details>'
    page=re.sub(r'<details><summary>40 separate tweet files</summary>.*?</details>',lambda _:tweet_section,page,flags=re.S)
    banner='<section style="border:1px solid #293b49;padding:22px;border-radius:12px;margin:20px 0"><h2>jobs, branches, and the garden</h2><p>40 rewritten tweets · 4 threads / 28 posts · 4 rewritten articles · six original matched visual pairs.</p><p>Release copy: hold until the public interface release and escrow deployment are verified. Merchant invoice adapters are a separate integration.</p><p><a href="release.json">release status</a> · <a href="threads/thread.01-a-home-for-your-agents.txt">intro thread</a> · <a href="threads/thread.02-one-budget-many-agents.txt">delegation thread</a> · <a href="threads/thread.03-what-counts-as-done.txt">acceptance thread</a> · <a href="threads/thread.04-reading-the-garden.txt">garden thread</a></p></section>'
    page=page.replace('<main>','<main>'+banner,1)
    (DEST/'index.html').write_text(page)
    archive(DEST/'header-suggestions.zip',DEST/'headers')
    archive(DEST/'pfp-suggestions.zip',DEST/'pfps')
    files={str(p.relative_to(DEST)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(DEST.rglob('*')) if p.is_file() and p.name!='checksums.json'}
    (DEST/'checksums.json').write_text(json.dumps(files,indent=2)+'\n')
    archive(BASE/f'{DEST.name}.zip',DEST)
    with zipfile.ZipFile(BASE/f'{DEST.name}.zip') as z:assert z.testzip() is None
    print(json.dumps({'archive':str(BASE/f'{DEST.name}.zip'),'files':len(files)+1,**status},indent=2))

if __name__=='__main__':main()
